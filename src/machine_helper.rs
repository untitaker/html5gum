use crate::utils::trace_log;
use crate::{Emitter, Reader, State, Tokenizer};

#[cfg(feature = "afl-ijon")]
use afl::{ijon_hashint, ijon_state};

#[derive(Debug)]
pub(crate) struct MachineState<R: Reader, E: Emitter> {
    #[allow(clippy::type_complexity)]
    pub function: fn(&mut Tokenizer<R, E>, Reconsume) -> Result<ControlToken<R, E>, R::Error>,
    #[cfg(debug_assertions)]
    pub debug_name: &'static str,
}

impl<R: Reader, E: Emitter> Copy for MachineState<R, E> {}
impl<R: Reader, E: Emitter> Clone for MachineState<R, E> {
    fn clone(&self) -> Self {
        *self
    }
}

/// A character that the next state has to consume first, before reading from the input:
/// `Some(Some(x))` for a byte, `Some(None)` for EOF, `None` for nothing.
///
/// The byte has already been preprocessed and validated, and the emitter's position is already
/// after it. So consuming it does not change the position.
#[allow(clippy::option_option)]
pub(crate) type Reconsume = Option<Option<u8>>;

pub(crate) enum ControlToken<R: Reader, E: Emitter> {
    Eof,
    Continue,
    /// Switch to the state, which then reconsumes the character, if any.
    SwitchTo(MachineState<R, E>, Reconsume),
}

impl<R: Reader, E: Emitter> ControlToken<R, E> {
    #[inline(always)]
    pub(crate) fn inline_next_state(
        self,
        tokenizer: &mut Tokenizer<R, E>,
    ) -> Result<Self, R::Error> {
        match self {
            ControlToken::SwitchTo(state, reconsume) => {
                tokenizer.machine_helper.switch_to(state);
                (state.function)(tokenizer, reconsume)
            }
            _ => {
                #[cfg(debug_assertions)]
                panic!("use of inline_next_state is invalid in this context as no state switch is happening");

                #[cfg(not(debug_assertions))]
                Ok(self)
            }
        }
    }
}

#[allow(clippy::from_over_into)]
impl<R: Reader, E: Emitter> Into<MachineState<R, E>> for State {
    fn into(self) -> MachineState<R, E> {
        // TODO: instead of this conversion, can we rig the enums to be of same layout?
        match self {
            State::Data => state_ref!(Data),
            State::PlainText => state_ref!(PlainText),
            State::RcData => state_ref!(RcData),
            State::RawText => state_ref!(RawText),
            State::ScriptData => state_ref!(ScriptData),
            State::CdataSection => state_ref!(CdataSection),
        }
    }
}

#[derive(Debug)]
pub(crate) struct MachineHelper<R: Reader, E: Emitter> {
    // XXX: allocation that cannot be controlled/reused by the user
    pub(crate) temporary_buffer: Vec<u8>,
    pub(crate) character_reference_code: u32,
    pub(crate) state: MachineState<R, E>,
    return_state: Option<(MachineState<R, E>, bool)>,
}

impl<R: Reader, E: Emitter> Default for MachineHelper<R, E> {
    fn default() -> Self {
        MachineHelper {
            temporary_buffer: Vec::new(),
            character_reference_code: 0,
            state: state_ref!(Data),
            return_state: None,
        }
    }
}

impl<R: Reader, E: Emitter> MachineHelper<R, E> {
    pub(crate) fn is_consumed_as_part_of_an_attribute(&self) -> bool {
        match self.return_state {
            Some((_state, is_attribute)) => is_attribute,
            None => false,
        }
    }

    pub(crate) fn flush_code_points_consumed_as_character_reference(&mut self, emitter: &mut E) {
        if self.is_consumed_as_part_of_an_attribute() {
            emitter.push_attribute_value(&self.temporary_buffer);
            self.temporary_buffer.clear();
        } else {
            self.flush_buffer_characters(emitter);
        }
    }

    pub(crate) fn flush_buffer_characters(&mut self, emitter: &mut E) {
        emitter.emit_string(&self.temporary_buffer);
        self.temporary_buffer.clear();
    }

    pub(crate) fn enter_state(&mut self, state: MachineState<R, E>, is_attribute: bool) {
        debug_assert!(self.return_state.is_none());
        let return_state = (self.state, is_attribute);
        self.return_state = Some(return_state);
        self.switch_to(state);
        #[cfg(feature = "afl-ijon")]
        ijon_state!(ijon_hashint(
            ijon_hashint(state.function as u32, return_state.0.function as u32),
            is_attribute as u32
        ));
    }

    pub(crate) fn pop_return_state(&mut self) -> MachineState<R, E> {
        self.return_state.take().unwrap().0
    }

    pub(crate) fn exit_state(&mut self) {
        let state = self.pop_return_state();
        self.switch_to(state);
    }

    pub(crate) fn switch_to(&mut self, state: MachineState<R, E>) {
        trace_log!(
            "switch_to: {} -> {}",
            self.state.debug_name,
            state.debug_name
        );
        self.state = state;
        #[cfg(feature = "afl-ijon")]
        ijon_state!(state.function as u32);
    }
}

macro_rules! state_ref {
    ($state:ident) => {{
        crate::machine_helper::MachineState {
            function: crate::machine::states::$state::run,
            #[cfg(debug_assertions)]
            debug_name: stringify!($state),
        }
    }};
}

pub(crate) use state_ref;

macro_rules! mutate_character_reference {
    ($slf:expr, * $mul:literal + $x:ident - $sub:literal) => {
        match $slf
            .machine_helper
            .character_reference_code
            .checked_mul($mul)
            .and_then(|cr| cr.checked_add($x as u32 - $sub))
        {
            Some(cr) => $slf.machine_helper.character_reference_code = cr,
            None => {
                // provoke err
                $slf.machine_helper.character_reference_code = 0x110000;
            }
        };
    };
}

pub(crate) use mutate_character_reference;

macro_rules! emit_current_tag_and_switch_to {
    ($slf:expr, $state:ident) => {{
        let state = $slf.emitter.emit_current_tag().map(Into::into);
        if state.is_some() {
            crate::utils::trace_log!("emitter asked for state switch:");
        }
        $slf.machine_helper
            .switch_to(state.unwrap_or($crate::machine_helper::state_ref!($state)));
        Ok(ControlToken::Continue)
    }};
}

pub(crate) use emit_current_tag_and_switch_to;

macro_rules! switch_to {
    ($slf:expr, $state:ident) => {{
        let new_state = $crate::machine_helper::state_ref!($state);
        Ok(ControlToken::SwitchTo(new_state, None))
    }};
}

pub(crate) use switch_to;

macro_rules! enter_state {
    ($slf:expr, $state:ident, $is_attribute:expr) => {{
        $slf.machine_helper
            .enter_state($crate::machine_helper::state_ref!($state), $is_attribute);
        Ok(ControlToken::Continue)
    }};
}

pub(crate) use enter_state;

macro_rules! exit_state {
    ($slf:expr) => {{
        $slf.machine_helper.exit_state();
        Ok(ControlToken::Continue)
    }};
}

pub(crate) use exit_state;

macro_rules! reconsume_in {
    ($slf:expr, $c:expr, $state:ident) => {{
        let new_state = $crate::machine_helper::state_ref!($state);
        let c: Option<u8> = $c;
        Ok(ControlToken::SwitchTo(new_state, Some(c)))
    }};
}

pub(crate) use reconsume_in;

/// Like `reconsume_in!(...)?.inline_next_state(slf)`, but with a direct call, which is faster.
///
/// Inlining the next state must not create a cycle of direct calls (or LLVM stops inlining).
///
/// `machine_helper.state` is not updated: the next state ends by switching states anyway (or by
/// returning `Eof`), so it must not depend on the current state, i.e. not use `enter_state!`.
macro_rules! reconsume_in_inline {
    ($slf:expr, $c:expr, $state:ident) => {{
        let c: Option<u8> = $c;
        crate::machine::states::$state::run($slf, Some(c))
    }};
}

pub(crate) use reconsume_in_inline;

macro_rules! reconsume_in_return_state {
    ($slf:expr, $c:expr) => {{
        let new_state = $slf.machine_helper.pop_return_state();
        let c: Option<u8> = $c;
        Ok(ControlToken::SwitchTo(new_state, Some(c)))
    }};
}

pub(crate) use reconsume_in_return_state;

/// Emit what comes before `c`, the character that was just read, at the position where `c`
/// starts. At EOF nothing was consumed, so there is nothing to step back over.
macro_rules! emit_before {
    ($slf:expr, $c:expr, $emit:expr) => {{
        let is_consumed = $c.is_some();
        if is_consumed {
            $slf.emitter.move_position(-1);
        }
        $emit;
        if is_consumed {
            $slf.emitter.move_position(1);
        }
    }};
}

pub(crate) use emit_before;

macro_rules! cont {
    () => {{
        continue;
    }};
}

pub(crate) use cont;

macro_rules! eof {
    () => {{
        Ok(ControlToken::Eof)
    }};
}

pub(crate) use eof;

macro_rules! read_byte {
    ($slf:expr) => {
        $slf.read_byte()
    };
}

pub(crate) use read_byte;

/// Produce error for current character. The error will be emitted once the character's bytes
/// have been fully consumed (and after any errors originating from pre-processing the input
/// stream bytes)
macro_rules! error {
    ($slf:expr, $e:expr) => {
        $slf.validator.set_character_error(&mut $slf.emitter, $e);
    };
}

pub(crate) use error;

/// Produce error for a previous character, emit immediately.
macro_rules! error_immediate {
    ($slf:expr, $e:expr) => {
        error!($slf, $e);
        $slf.validator.flush_character_error(&mut $slf.emitter);
    };
}

pub(crate) use error_immediate;
