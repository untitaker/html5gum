//! Consume the parsed HTML as a series of events through a callback.
//!
//! While using the [crate::DefaultEmitter] provides an easy-to-use API with low performance, and
//! implementing your own [crate::Emitter] brings maximal performance and maximal pain, this is a middle
//! ground. All strings are borrowed from some intermediate buffer instead of individually
//! allocated.
//!
//! ```
//! // Extract all text between span tags, in a naive (but fast) way. Does not handle tags inside of the span. See `examples/` as well.
//! use html5gum::Tokenizer;
//! use html5gum::emitters::callback::{CallbackEvent, CallbackEmitter};
//!
//! let mut is_in_span = false;
//! let emitter = CallbackEmitter::new(move |event: CallbackEvent<'_>| -> Option<Vec<u8>> {
//!     match event {
//!         CallbackEvent::OpenStartTag { name } => {
//!             is_in_span = name == b"span";
//!         },
//!         CallbackEvent::String { value } if is_in_span => {
//!             return Some(value.to_vec());
//!         }
//!         CallbackEvent::EndTag { .. } => {
//!             is_in_span = false;
//!         }
//!         _ => {}
//!     }
//!
//!     None
//! });
//!
//! let input = r#"<h1><span class=hello>Hello</span> world!</h1>"#;
//! let Ok(text_fragments) = Tokenizer::new_with_emitter(input, emitter)
//!     .collect::<Result<Vec<_>, _>>();
//!
//! assert_eq!(text_fragments, vec![b"Hello".to_vec()]);
//! ```

use std::collections::VecDeque;
use std::convert::Infallible;
use std::mem::swap;

use crate::utils::trace_log;
use crate::{naive_next_state, Emitter, Error, State};

/// Events used by [CallbackEmitter].
///
/// This operates at a slightly lower level than [crate::Token], as start tags are split up into multiple
/// events.
#[derive(Debug)]
pub enum CallbackEvent<'a> {
    /// Visit the `"<mytag"` in `"<mytag mykey=myvalue>"`. Signifies the beginning of a new start
    /// tag.
    ///
    /// Attributes have not yet been read.
    OpenStartTag {
        /// The name of the start tag.
        name: &'a [u8],
    },

    /// Visit an attribute name, for example `"mykey"` in `"<mytag mykey=myvalue>"`.
    ///
    /// The attribute value has not yet been read.
    AttributeName {
        /// The name of the attribute.
        name: &'a [u8],
    },

    /// Visit an attribute value, for example `"myvalue"` in `"<mytag mykey=myvalue>"`.
    ///
    /// Things like whitespace, quote handling is taken care of.
    ///
    /// After this event, the start tag may be closed using `CloseStartTag`, or another
    /// `AttributeName` may follow.
    AttributeValue {
        /// The value of the attribute.
        value: &'a [u8],
    },

    /// Visit the end of the start tag, for example `">"` in `"<mytag mykey=myvalue>"`.
    ///
    CloseStartTag {
        /// Whether the tag ended with `"/>"`.
        ///
        /// Note that in HTML5 this difference is largely ignored, and tags are considered
        /// self-closing based on a hardcoded list of names, not based on syntax.
        self_closing: bool,
    },

    /// Visit `"</mytag>"`.
    ///
    /// Note: Because of strangeness in the HTML spec, attributes may be observed outside of start
    /// tags, before this event. It's best to ignore them as they are not valid HTML, but can still
    /// be observed through most HTML parsers.
    EndTag {
        /// The name of the end tag.
        name: &'a [u8],
    },

    /// Visit a string, as in, the actual text between tags. The content. Remember actual content
    /// in HTML, before SPAs took over? I remember.
    ///
    /// It's guaranteed that all consecutive "character tokens" (as the spec calls them) are folded
    /// into one string event.
    String {
        /// A series of character tokens.
        value: &'a [u8],
    },

    /// Visit a comment, like `<!-- DON'T HACK THIS WEBSITE -->`
    Comment {
        /// The contents of the comment.
        value: &'a [u8],
    },

    /// Visit `<!DOCTYPE html>`.
    Doctype {
        /// Name of the docstring.
        name: &'a [u8],
        /// Public identifier (see spec)
        public_identifier: Option<&'a [u8]>,
        /// System identifier (see spec)
        system_identifier: Option<&'a [u8]>,
        /// Enable quirksmode
        force_quirks: bool,
    },

    /// Visit a parsing error.
    Error(Error),
}

#[derive(Debug, Clone, Copy)]
enum CurrentTag {
    Start,
    End,
}

#[derive(Debug)]
struct CallbackState<F, T> {
    callback: F,
    emitted_tokens: VecDeque<T>,
}

/// This trait is implemented for all functions that have the same signature as
/// [Callback::handle_event]. The trait only exists in case you want to implement it on a nameable
/// type.
pub trait Callback<T> {
    /// Perform some action on a parsing event, and, optionally, return a value that can be yielded
    /// from the [crate::Tokenizer] iterator.
    fn handle_event(&mut self, event: CallbackEvent<'_>) -> Option<T>;
}

impl<T, F> Callback<T> for F
where
    F: FnMut(CallbackEvent<'_>) -> Option<T>,
{
    fn handle_event(&mut self, event: CallbackEvent<'_>) -> Option<T> {
        self(event)
    }
}

impl<F, T> CallbackState<F, T>
where
    F: Callback<T>,
{
    fn emit_event(&mut self, event: CallbackEvent<'_>) {
        let res = self.callback.handle_event(event);
        if let Some(token) = res {
            self.emitted_tokens.push_front(token);
        }
    }
}

impl<F, T> Default for CallbackState<F, T>
where
    F: Default,
{
    fn default() -> Self {
        CallbackState {
            callback: F::default(),
            emitted_tokens: VecDeque::default(),
        }
    }
}

#[derive(Debug, Default)]
struct EmitterState {
    naively_switch_states: bool,

    current_characters: Vec<u8>,
    current_comment: Vec<u8>,

    last_start_tag: Vec<u8>,
    current_tag_had_attributes: bool,
    current_tag_type: Option<CurrentTag>,
    current_tag_self_closing: bool,
    current_tag_name: Vec<u8>,
    current_attribute_name: Vec<u8>,
    current_attribute_value: Vec<u8>,

    // strings related to doctype
    doctype_name: Vec<u8>,
    doctype_has_public_identifier: bool,
    doctype_has_system_identifier: bool,
    doctype_public_identifier: Vec<u8>,
    doctype_system_identifier: Vec<u8>,
    doctype_force_quirks: bool,
}

/// The emitter class to pass to [crate::Tokenizer::new_with_emitter]. Please refer to the
/// module-level documentation on [crate::emitters::callback] for usage.
#[derive(Debug)]
pub struct CallbackEmitter<F, T = Infallible> {
    // this struct is only split out so [CallbackState::emit_event] can borrow things concurrently
    // with other attributes.
    callback_state: CallbackState<F, T>,
    emitter_state: EmitterState,
}

impl<F, T> Default for CallbackEmitter<F, T>
where
    F: Default,
{
    fn default() -> Self {
        CallbackEmitter {
            callback_state: CallbackState::default(),
            emitter_state: EmitterState::default(),
        }
    }
}

impl<F, T> CallbackEmitter<F, T>
where
    F: Callback<T>,
{
    /// Create a new emitter.
    ///
    /// The given callback may return optional tokens that then become available through the
    /// [crate::Tokenizer]'s iterator. If that's not used, return `Option<Infallible>`.
    pub fn new(callback: F) -> Self {
        CallbackEmitter {
            callback_state: CallbackState {
                callback,
                emitted_tokens: VecDeque::new(),
            },
            emitter_state: EmitterState::default(),
        }
    }

    /// Get mutable access to the inner callback.
    pub fn callback_mut(&mut self) -> &mut F {
        &mut self.callback_state.callback
    }

    /// Whether to use [`naive_next_state`] to switch states automatically.
    ///
    /// The default is off.
    pub fn naively_switch_states(&mut self, yes: bool) {
        self.emitter_state.naively_switch_states = yes;
    }

    fn flush_attribute_name(&mut self) {
        if !self.emitter_state.current_attribute_name.is_empty() {
            self.callback_state
                .emit_event(CallbackEvent::AttributeName {
                    name: &self.emitter_state.current_attribute_name,
                });
            self.emitter_state.current_attribute_name.clear();
        }
    }

    fn flush_attribute(&mut self) {
        self.flush_attribute_name();

        if !self.emitter_state.current_attribute_value.is_empty() {
            self.callback_state
                .emit_event(CallbackEvent::AttributeValue {
                    value: &self.emitter_state.current_attribute_value,
                });
            self.emitter_state.current_attribute_value.clear();
        }
    }

    fn flush_open_start_tag(&mut self) {
        if matches!(self.emitter_state.current_tag_type, Some(CurrentTag::Start))
            && !self.emitter_state.current_tag_name.is_empty()
        {
            self.callback_state.emit_event(CallbackEvent::OpenStartTag {
                name: &self.emitter_state.current_tag_name,
            });

            self.emitter_state.last_start_tag.clear();
            swap(
                &mut self.emitter_state.last_start_tag,
                &mut self.emitter_state.current_tag_name,
            );
        }
    }

    fn flush_current_characters(&mut self) {
        if self.emitter_state.current_characters.is_empty() {
            return;
        }

        self.callback_state.emit_event(CallbackEvent::String {
            value: &self.emitter_state.current_characters,
        });
        self.emitter_state.current_characters.clear();
    }
}
impl<F, T> Emitter for CallbackEmitter<F, T>
where
    F: Callback<T>,
{
    type Token = T;

    fn set_last_start_tag(&mut self, last_start_tag: Option<&[u8]>) {
        self.emitter_state.last_start_tag.clear();
        self.emitter_state
            .last_start_tag
            .extend(last_start_tag.unwrap_or_default());
    }

    fn emit_eof(&mut self) {
        self.flush_current_characters();
    }

    fn emit_error(&mut self, error: Error) {
        self.callback_state.emit_event(CallbackEvent::Error(error));
    }

    fn pop_token(&mut self) -> Option<Self::Token> {
        self.callback_state.emitted_tokens.pop_back()
    }

    fn emit_string(&mut self, s: &[u8]) {
        crate::utils::trace_log!("callbacks: emit_string, len={}", s.len());
        self.emitter_state.current_characters.extend(s);
    }

    fn init_start_tag(&mut self) {
        self.emitter_state.current_tag_name.clear();
        self.emitter_state.current_tag_type = Some(CurrentTag::Start);
        self.emitter_state.current_tag_self_closing = false;
    }

    fn init_end_tag(&mut self) {
        self.emitter_state.current_tag_name.clear();
        self.emitter_state.current_tag_type = Some(CurrentTag::End);
        self.emitter_state.current_tag_had_attributes = false;
    }

    fn init_comment(&mut self) {
        self.flush_current_characters();
        self.emitter_state.current_comment.clear();
    }

    fn emit_current_tag(&mut self) -> Option<State> {
        self.flush_attribute();
        self.flush_current_characters();
        match self.emitter_state.current_tag_type {
            Some(CurrentTag::Start) => {
                self.flush_open_start_tag();
                self.callback_state
                    .emit_event(CallbackEvent::CloseStartTag {
                        self_closing: self.emitter_state.current_tag_self_closing,
                    });
            }
            Some(CurrentTag::End) => {
                if self.emitter_state.current_tag_had_attributes {
                    self.emit_error(Error::EndTagWithAttributes);
                }
                self.emitter_state.last_start_tag.clear();
                self.callback_state.emit_event(CallbackEvent::EndTag {
                    name: &self.emitter_state.current_tag_name,
                });
            }
            _ => {}
        }

        if self.emitter_state.naively_switch_states {
            naive_next_state(&self.emitter_state.last_start_tag)
        } else {
            None
        }
    }
    fn emit_current_comment(&mut self) {
        self.callback_state.emit_event(CallbackEvent::Comment {
            value: &self.emitter_state.current_comment,
        });
        self.emitter_state.current_comment.clear();
    }

    fn emit_current_doctype(&mut self) {
        self.callback_state.emit_event(CallbackEvent::Doctype {
            name: &self.emitter_state.doctype_name,
            public_identifier: if self.emitter_state.doctype_has_public_identifier {
                Some(&self.emitter_state.doctype_public_identifier)
            } else {
                None
            },
            system_identifier: if self.emitter_state.doctype_has_system_identifier {
                Some(&self.emitter_state.doctype_system_identifier)
            } else {
                None
            },
            force_quirks: self.emitter_state.doctype_force_quirks,
        });
    }

    fn set_self_closing(&mut self) {
        trace_log!("set_self_closing");
        if matches!(self.emitter_state.current_tag_type, Some(CurrentTag::End)) {
            self.callback_state
                .emit_event(CallbackEvent::Error(Error::EndTagWithTrailingSolidus));
        } else {
            self.emitter_state.current_tag_self_closing = true;
        }
    }

    fn set_force_quirks(&mut self) {
        self.emitter_state.doctype_force_quirks = true;
    }

    fn push_tag_name(&mut self, s: &[u8]) {
        self.emitter_state.current_tag_name.extend(s);
    }

    fn push_comment(&mut self, s: &[u8]) {
        self.emitter_state.current_comment.extend(s);
    }

    fn push_doctype_name(&mut self, s: &[u8]) {
        self.emitter_state.doctype_name.extend(s);
    }

    fn init_doctype(&mut self) {
        self.flush_current_characters();
        self.emitter_state.doctype_name.clear();
        self.emitter_state.doctype_has_public_identifier = false;
        self.emitter_state.doctype_has_system_identifier = false;
        self.emitter_state.doctype_public_identifier.clear();
        self.emitter_state.doctype_system_identifier.clear();
        self.emitter_state.doctype_force_quirks = false;
    }

    fn init_attribute(&mut self) {
        self.flush_open_start_tag();
        self.flush_attribute();
        self.emitter_state.current_tag_had_attributes = true;
    }

    fn push_attribute_name(&mut self, s: &[u8]) {
        self.emitter_state.current_attribute_name.extend(s);
    }

    fn push_attribute_value(&mut self, s: &[u8]) {
        self.flush_attribute_name();
        self.emitter_state.current_attribute_value.extend(s);
    }

    fn set_doctype_public_identifier(&mut self, value: &[u8]) {
        self.emitter_state.doctype_has_public_identifier = true;
        self.emitter_state.doctype_public_identifier.clear();
        self.emitter_state.doctype_public_identifier.extend(value);
    }
    fn set_doctype_system_identifier(&mut self, value: &[u8]) {
        self.emitter_state.doctype_has_system_identifier = true;
        self.emitter_state.doctype_system_identifier.clear();
        self.emitter_state.doctype_system_identifier.extend(value);
    }
    fn push_doctype_public_identifier(&mut self, value: &[u8]) {
        self.emitter_state.doctype_public_identifier.extend(value);
    }
    fn push_doctype_system_identifier(&mut self, value: &[u8]) {
        self.emitter_state.doctype_system_identifier.extend(value);
    }

    fn current_is_appropriate_end_tag_token(&mut self) -> bool {
        if self.emitter_state.last_start_tag.is_empty() {
            crate::utils::trace_log!(
                "current_is_appropriate_end_tag_token: no, because last_start_tag is empty"
            );
            return false;
        }

        if !matches!(self.emitter_state.current_tag_type, Some(CurrentTag::End)) {
            crate::utils::trace_log!(
                "current_is_appropriate_end_tag_token: no, because current_tag_type is not end"
            );
            return false;
        }

        crate::utils::trace_log!(
            "current_is_appropriate_end_tag_token: last_start_tag = {:?}",
            self.emitter_state.last_start_tag
        );
        crate::utils::trace_log!(
            "current_is_appropriate_end_tag_token: current_tag = {:?}",
            self.emitter_state.current_tag_name
        );
        self.emitter_state.last_start_tag == self.emitter_state.current_tag_name
    }
}
