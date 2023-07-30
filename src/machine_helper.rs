use crate::utils::{ControlToken, trace_log};
use crate::{Reader, Emitter, State, Tokenizer};

pub(crate) type MachineState<R, E> = fn(&mut Tokenizer<R, E>) -> Result<ControlToken, <R as Reader>::Error>;

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
    return_state: Option<MachineState<R, E>>,
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
            Some(state) => state == state_ref!(AttributeValueDoubleQuoted) || state == state_ref!(AttributeValueSingleQuoted) || state == state_ref!(AttributeValueUnquoted),
            None => false,
        }
    }

    pub(crate) fn flush_code_points_consumed_as_character_reference(
        &mut self,
        emitter: &mut E,
    ) {
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

    pub(crate) fn enter_state(&mut self, state: MachineState<R, E>) {
        debug_assert!(self.return_state.is_none());
        self.return_state = Some(self.state);
        self.switch_to(state);
    }

    pub(crate) fn pop_return_state(&mut self) -> MachineState<R, E> {
        self.return_state.take().unwrap()
    }

    pub(crate) fn exit_state(&mut self) {
        let state = self.pop_return_state();
        self.switch_to(state);
    }

    pub(crate) fn switch_to(&mut self, state: MachineState<R, E>) {
        trace_log!("switch_to: {:?} -> {:?}", self.state, state);
        self.state = state;
    }
}

macro_rules! state_ref {
    ($state:ident) => {
        crate::machine::states::$state::run
    }
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
        $slf.machine_helper.switch_to(state.unwrap_or($crate::machine_helper::state_ref!($state)));
        Ok(ControlToken::Continue)
    }};
}

pub(crate) use emit_current_tag_and_switch_to;

macro_rules! switch_to {
    ($slf:expr, $state:ident) => {{
        let new_state = $crate::machine_helper::state_ref!($state);
        $slf.machine_helper.switch_to(new_state);
        Ok(ControlToken::Continue)
    }};
}

pub(crate) use switch_to;

macro_rules! enter_state {
    ($slf:expr, $state:ident) => {{
        $slf.machine_helper.enter_state($crate::machine_helper::state_ref!($state));
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
        let c = $c;
        $slf.reader.unread_byte(c);
        $slf.machine_helper.switch_to(new_state);
        // call state directly so that it gets inlined into current fn
        new_state($slf)
    }};
}

pub(crate) use reconsume_in;

macro_rules! reconsume_in_return_state {
    ($slf:expr, $c:expr) => {{
        let new_state = $slf.machine_helper.pop_return_state();
        let c = $c;
        $slf.reader.unread_byte(c);
        $slf.machine_helper.switch_to(new_state);
        // call state directly so that it gets inlined into current fn
        new_state($slf)
    }}
}

pub(crate) use reconsume_in_return_state;


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
        $slf.reader
            .read_byte(&mut $slf.validator, &mut $slf.emitter)
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
    }
}

pub(crate) use error_immediate;
