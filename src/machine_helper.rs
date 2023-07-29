use crate::state::MachineState as State;
use crate::utils::trace_log;
use crate::Emitter;

#[derive(Debug)]
pub(crate) struct MachineHelper {
    // XXX: allocation that cannot be controlled/reused by the user
    pub(crate) temporary_buffer: Vec<u8>,
    pub(crate) character_reference_code: u32,
    pub(crate) state: State,
    return_state: Option<State>,
}

impl Default for MachineHelper {
    fn default() -> Self {
        MachineHelper {
            temporary_buffer: Vec::new(),
            character_reference_code: 0,
            state: State::Data,
            return_state: None,
        }
    }
}

impl MachineHelper {
    pub(crate) fn is_consumed_as_part_of_an_attribute(&self) -> bool {
        matches!(
            self.return_state,
            Some(
                State::AttributeValueDoubleQuoted
                    | State::AttributeValueSingleQuoted
                    | State::AttributeValueUnquoted
            )
        )
    }
    pub(crate) fn flush_code_points_consumed_as_character_reference<E: Emitter>(
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
    pub(crate) fn flush_buffer_characters<E: Emitter>(&mut self, emitter: &mut E) {
        emitter.emit_string(&self.temporary_buffer);
        self.temporary_buffer.clear();
    }

    pub(crate) fn enter_state(&mut self, state: State) {
        debug_assert!(self.return_state.is_none());
        self.return_state = Some(self.state);
        self.switch_to(state);
    }

    pub(crate) fn pop_return_state(&mut self) -> State {
        self.return_state.take().unwrap()
    }

    pub(crate) fn exit_state(&mut self) {
        let state = self.pop_return_state();
        self.switch_to(state);
    }

    pub(crate) fn state(&self) -> State {
        self.state
    }

    pub(crate) fn switch_to(&mut self, state: State) {
        trace_log!("switch_to: {:?} -> {:?}", self.state, state);
        self.state = state;
    }
}

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
    ($slf:expr, $state:expr) => {{
        let state = $slf.emitter.emit_current_tag().map(From::from);
        switch_to!($slf, state.unwrap_or($state))
    }};
}

pub(crate) use emit_current_tag_and_switch_to;

macro_rules! switch_to {
    ($slf:expr, $state:expr) => {{
        $slf.machine_helper.switch_to($state);
        Ok(ControlToken::Continue)
    }};
}

pub(crate) use switch_to;

macro_rules! enter_state {
    ($slf:expr, $state:expr) => {{
        $slf.machine_helper.enter_state($state);
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
    ($slf:expr, $c:expr, $state:expr) => {{
        let new_state = $state;
        let c = $c;
        $slf.reader.unread_byte(c);
        $slf.machine_helper.switch_to(new_state);
        Ok(ControlToken::Continue)
    }};
}

pub(crate) use reconsume_in;

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
    };
}

pub(crate) use error_immediate;
