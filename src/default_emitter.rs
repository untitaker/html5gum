use std::collections::BTreeMap;
use std::mem::take;

use crate::{Emitter, Error, HtmlString, State};

use crate::callbacks::{Callback, CallbackEmitter, CallbackEvent};

#[derive(Debug, Default)]
struct OurCallback {
    tag_name: Vec<u8>,
    attribute_name: HtmlString,
    attribute_map: BTreeMap<HtmlString, HtmlString>,
}

impl Callback<Token> for OurCallback {
    fn handle_event(&mut self, event: CallbackEvent<'_>) -> Option<Token> {
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.tag_name.clear();
                self.tag_name.extend(name);
                None
            }
            CallbackEvent::AttributeName { name } => {
                self.attribute_name.clear();
                self.attribute_name.extend(name);
                self.attribute_map
                    .insert(name.to_owned().into(), Default::default());
                None
            }
            CallbackEvent::AttributeValue { value } => {
                self.attribute_map
                    .get_mut(&self.attribute_name)
                    .unwrap()
                    .extend(value);
                None
            }
            CallbackEvent::CloseStartTag { self_closing } => Some(Token::StartTag(StartTag {
                self_closing,
                name: take(&mut self.tag_name).into(),
                attributes: take(&mut self.attribute_map),
            })),
            CallbackEvent::EndTag { name } => Some(Token::EndTag(EndTag {
                name: name.to_owned().into(),
            })),
            CallbackEvent::String { value } => Some(Token::String(value.to_owned().into())),
            CallbackEvent::Comment { value } => Some(Token::Comment(value.to_owned().into())),
            CallbackEvent::Doctype {
                name,
                public_identifier,
                system_identifier,
                force_quirks,
            } => Some(Token::Doctype(Doctype {
                force_quirks,
                name: name.to_owned().into(),
                public_identifier: Some(public_identifier.to_owned().into()),
                system_identifier: Some(system_identifier.to_owned().into()),
            })),
            CallbackEvent::Error(error) => Some(Token::Error(error)),
        }
    }
}

/// The default implementation of [`crate::Emitter`], used to produce ("emit") tokens.
#[derive(Default, Debug)]
pub struct DefaultEmitter {
    inner: CallbackEmitter<OurCallback, Token>,
}

impl DefaultEmitter {
    /// Whether to use [`naive_next_state`] to switch states automatically.
    ///
    /// The default is off.
    pub fn naively_switch_states(&mut self, yes: bool) {
        self.inner.naively_switch_states(yes)
    }
}

// opaque type around inner emitter
impl Emitter for DefaultEmitter {
    type Token = Token;

    fn set_last_start_tag(&mut self, last_start_tag: Option<&[u8]>) {
        self.inner.set_last_start_tag(last_start_tag)
    }

    fn emit_eof(&mut self) {
        self.inner.emit_eof()
    }

    fn emit_error(&mut self, error: Error) {
        self.inner.emit_error(error)
    }

    fn should_emit_errors(&mut self) -> bool {
        self.inner.should_emit_errors()
    }

    fn pop_token(&mut self) -> Option<Self::Token> {
        self.inner.pop_token()
    }
    fn emit_string(&mut self, c: &[u8]) {
        self.inner.emit_string(c)
    }

    fn init_start_tag(&mut self) {
        self.inner.init_start_tag()
    }

    fn init_end_tag(&mut self) {
        self.inner.init_end_tag()
    }

    fn init_comment(&mut self) {
        self.inner.init_comment()
    }

    fn emit_current_tag(&mut self) -> Option<State> {
        self.inner.emit_current_tag()
    }

    fn emit_current_comment(&mut self) {
        self.inner.emit_current_comment()
    }

    fn emit_current_doctype(&mut self) {
        self.inner.emit_current_doctype()
    }

    fn set_self_closing(&mut self) {
        self.inner.set_self_closing()
    }

    fn set_force_quirks(&mut self) {
        self.inner.set_force_quirks()
    }

    fn push_tag_name(&mut self, s: &[u8]) {
        self.inner.push_tag_name(s)
    }

    fn push_comment(&mut self, s: &[u8]) {
        self.inner.push_comment(s)
    }

    fn push_doctype_name(&mut self, s: &[u8]) {
        self.inner.push_doctype_name(s)
    }

    fn init_doctype(&mut self) {
        self.inner.init_doctype()
    }

    fn init_attribute(&mut self) {
        self.inner.init_attribute()
    }

    fn push_attribute_name(&mut self, s: &[u8]) {
        self.inner.push_attribute_name(s)
    }

    fn push_attribute_value(&mut self, s: &[u8]) {
        self.inner.push_attribute_value(s)
    }

    fn set_doctype_public_identifier(&mut self, value: &[u8]) {
        self.inner.set_doctype_public_identifier(value)
    }

    fn set_doctype_system_identifier(&mut self, value: &[u8]) {
        self.inner.set_doctype_system_identifier(value)
    }

    fn push_doctype_public_identifier(&mut self, s: &[u8]) {
        self.inner.push_doctype_public_identifier(s)
    }

    fn push_doctype_system_identifier(&mut self, s: &[u8]) {
        self.inner.push_doctype_system_identifier(s)
    }

    fn current_is_appropriate_end_tag_token(&mut self) -> bool {
        self.inner.current_is_appropriate_end_tag_token()
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&mut self) -> bool {
        self.inner
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

/// A HTML end/close tag, such as `<p>` or `<a>`.
#[derive(Debug, Default, Eq, PartialEq, Clone)]
pub struct StartTag {
    /// Whether this tag is self-closing. If it is self-closing, no following [`EndTag`] should be
    /// expected.
    pub self_closing: bool,

    /// The start tag's name, such as `"p"` or `"a"`.
    pub name: HtmlString,

    /// A mapping for any HTML attributes this start tag may have.
    ///
    /// Duplicate attributes are ignored after the first one as per WHATWG spec. Implement your own
    /// [`Emitter`] to tweak this behavior.
    pub attributes: BTreeMap<HtmlString, HtmlString>,
}

/// A HTML end/close tag, such as `</p>` or `</a>`.
#[derive(Debug, Default, Eq, PartialEq, Clone)]
pub struct EndTag {
    /// The ending tag's name, such as `"p"` or `"a"`.
    pub name: HtmlString,
}

/// A doctype. Some examples:
///
/// * `<!DOCTYPE {name}>`
/// * `<!DOCTYPE {name} PUBLIC '{public_identifier}'>`
/// * `<!DOCTYPE {name} SYSTEM '{system_identifier}'>`
/// * `<!DOCTYPE {name} PUBLIC '{public_identifier}' '{system_identifier}'>`
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Doctype {
    /// The ["force quirks"](https://html.spec.whatwg.org/#force-quirks-flag) flag.
    pub force_quirks: bool,

    /// The doctype's name. For HTML documents this is "html".
    pub name: HtmlString,

    /// The doctype's public identifier.
    pub public_identifier: Option<HtmlString>,

    /// The doctype's system identifier.
    pub system_identifier: Option<HtmlString>,
}

/// The token type used by default. You can define your own token type by implementing the
/// [`crate::Emitter`] trait and using [`crate::Tokenizer::new_with_emitter`].
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Token {
    /// A HTML start tag.
    StartTag(StartTag),
    /// A HTML end tag.
    EndTag(EndTag),
    /// A literal string.
    String(HtmlString),
    /// A HTML comment.
    Comment(HtmlString),
    /// A HTML doctype declaration.
    Doctype(Doctype),
    /// A HTML parsing error.
    ///
    /// Can be skipped over, the tokenizer is supposed to recover from the error and continues with
    /// more tokens afterward.
    Error(Error),
}
