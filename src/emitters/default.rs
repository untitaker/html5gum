//! The default emitter is what powers the simple SAX-like API that you see in the README.
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::mem::take;

use crate::span::SpanBoundFromReader;
use crate::{Emitter, Error, HtmlString, Reader, Span, SpanBound, Spanned, State};

use crate::emitters::callback::{Callback, CallbackEmitter, CallbackEvent};

#[derive(Debug, Default)]
struct OurCallback<S: SpanBound> {
    tag_name: Vec<u8>,
    tag_start_span: S,
    attribute_name: Spanned<HtmlString, S>,
    attribute_map: BTreeMap<HtmlString, Spanned<HtmlString, S>>,
}

impl<R: Reader, S: SpanBound> Callback<Token<S>, S, R> for OurCallback<S> {
    fn handle_event(
        &mut self,
        event: CallbackEvent<'_>,
        span: Span<S>,
        _reader: &R,
    ) -> Option<Token<S>> {
        crate::utils::trace_log!("event: {:?}", event);
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.tag_name.clear();
                self.tag_name.extend(name);
                self.tag_start_span = span.start;
                None
            }
            CallbackEvent::AttributeName { name } => {
                self.attribute_name.clear();
                match self.attribute_map.entry(name.to_owned().into()) {
                    Entry::Occupied(old) => Some(Token::Error(Spanned {
                        value: Error::DuplicateAttribute,
                        span: old.get().span,
                    })),
                    Entry::Vacant(vacant) => {
                        self.attribute_name.extend(name);
                        vacant.insert(Spanned {
                            value: Default::default(),
                            span,
                        });
                        None
                    }
                }
            }
            CallbackEvent::AttributeValue { value } => {
                if !self.attribute_name.is_empty() {
                    let attr = self.attribute_map.get_mut(&*self.attribute_name).unwrap();
                    attr.extend(value);
                    attr.span.end = span.end.offset(1);
                }
                None
            }
            CallbackEvent::CloseStartTag { self_closing } => Some(Token::StartTag(StartTag {
                self_closing,
                name: take(&mut self.tag_name).into(),
                span: Span {
                    start: self.tag_start_span,
                    end: span.end,
                },
                attributes: take(&mut self.attribute_map),
            })),
            CallbackEvent::EndTag { name } => {
                self.attribute_map.clear();
                Some(Token::EndTag(EndTag {
                    name: name.to_owned().into(),
                    span,
                }))
            }
            CallbackEvent::String { value } => Some(Token::String(Spanned {
                value: value.to_owned().into(),
                span,
            })),
            CallbackEvent::Comment { value } => Some(Token::Comment(Spanned {
                value: value.to_owned().into(),
                span,
            })),
            CallbackEvent::Doctype {
                name,
                public_identifier,
                system_identifier,
                force_quirks,
            } => Some(Token::Doctype(Spanned {
                value: Doctype {
                    force_quirks,
                    name: name.to_owned().into(),
                    public_identifier: public_identifier.map(|x| x.to_owned().into()),
                    system_identifier: system_identifier.map(|x| x.to_owned().into()),
                },
                span,
            })),
            CallbackEvent::Error(error) => Some(Token::Error(Spanned { value: error, span })),
        }
    }
}

/// This is the emitter you implicitly use with [crate::Tokenizer::new]. Refer to the [crate
/// docs](crate) for how usage looks like.
#[derive(Debug)]
pub struct DefaultEmitter<R: Reader, S: SpanBoundFromReader<R> = ()> {
    inner: CallbackEmitter<OurCallback<S>, R, Token<S>, S>,
}

impl<R: Reader> Default for DefaultEmitter<R, ()> {
    fn default() -> Self {
        Self {
            inner: Default::default(),
        }
    }
}

impl<S: SpanBoundFromReader<R>, R: Reader> DefaultEmitter<R, S> {
    /// Create a new [`DefaultEmitter`] for a certain [`Span`]type which you can pass to
    /// [`crate::Tokenizer::new_with_emitter`].
    #[must_use]
    pub fn new_with_span() -> Self {
        Self {
            inner: Default::default(),
        }
    }

    /// Whether to use [crate::naive_next_state] to switch states automatically.
    ///
    /// The default is off.
    pub fn naively_switch_states(&mut self, yes: bool) {
        self.inner.naively_switch_states(yes)
    }
}

impl<S: SpanBoundFromReader<R>, R: Reader> Emitter<R> for DefaultEmitter<R, S> {
    type Token = Token<S>;

    // opaque type around inner emitter

    fn set_last_start_tag(&mut self, last_start_tag: Option<&[u8]>, reader: &R) {
        self.inner.set_last_start_tag(last_start_tag, reader)
    }

    fn emit_eof(&mut self, reader: &R) {
        self.inner.emit_eof(reader)
    }

    fn emit_error(&mut self, error: Error, reader: &R) {
        self.inner.emit_error(error, reader)
    }

    fn should_emit_errors(&mut self) -> bool {
        self.inner.should_emit_errors()
    }

    fn pop_token(&mut self, reader: &R) -> Option<Self::Token> {
        self.inner.pop_token(reader)
    }
    fn emit_string(&mut self, c: &[u8], reader: &R) {
        self.inner.emit_string(c, reader)
    }

    fn init_start_tag(&mut self, reader: &R) {
        self.inner.init_start_tag(reader)
    }

    fn init_end_tag(&mut self, reader: &R) {
        self.inner.init_end_tag(reader)
    }

    fn init_comment(&mut self, reader: &R) {
        self.inner.init_comment(reader)
    }

    fn emit_current_tag(&mut self, reader: &R) -> Option<State> {
        self.inner.emit_current_tag(reader)
    }

    fn emit_current_comment(&mut self, reader: &R) {
        self.inner.emit_current_comment(reader)
    }

    fn emit_current_doctype(&mut self, reader: &R) {
        self.inner.emit_current_doctype(reader)
    }

    fn set_self_closing(&mut self, reader: &R) {
        self.inner.set_self_closing(reader)
    }

    fn set_force_quirks(&mut self, reader: &R) {
        self.inner.set_force_quirks(reader)
    }

    fn push_tag_name(&mut self, s: &[u8], reader: &R) {
        self.inner.push_tag_name(s, reader)
    }

    fn push_comment(&mut self, s: &[u8], reader: &R) {
        self.inner.push_comment(s, reader)
    }

    fn push_doctype_name(&mut self, s: &[u8], reader: &R) {
        self.inner.push_doctype_name(s, reader)
    }

    fn init_doctype(&mut self, reader: &R) {
        self.inner.init_doctype(reader)
    }

    fn init_attribute(&mut self, reader: &R) {
        self.inner.init_attribute(reader)
    }

    fn push_attribute_name(&mut self, s: &[u8], reader: &R) {
        self.inner.push_attribute_name(s, reader)
    }

    fn push_attribute_value(&mut self, s: &[u8], reader: &R) {
        self.inner.push_attribute_value(s, reader)
    }

    fn set_doctype_public_identifier(&mut self, value: &[u8], reader: &R) {
        self.inner.set_doctype_public_identifier(value, reader)
    }

    fn set_doctype_system_identifier(&mut self, value: &[u8], reader: &R) {
        self.inner.set_doctype_system_identifier(value, reader)
    }

    fn push_doctype_public_identifier(&mut self, s: &[u8], reader: &R) {
        self.inner.push_doctype_public_identifier(s, reader)
    }

    fn push_doctype_system_identifier(&mut self, s: &[u8], reader: &R) {
        self.inner.push_doctype_system_identifier(s, reader)
    }

    fn current_is_appropriate_end_tag_token(&mut self) -> bool {
        self.inner.current_is_appropriate_end_tag_token()
    }

    fn start_open_tag(&mut self, reader: &R) {
        self.inner.start_open_tag(reader)
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&mut self) -> bool {
        self.inner
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

/// A HTML end/close tag, such as `<p>` or `<a>`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartTag<S: SpanBound> {
    /// Whether this tag is self-closing. If it is self-closing, no following [EndTag] should be
    /// expected.
    pub self_closing: bool,

    /// The start tag's name, such as `"p"` or `"a"`.
    pub name: HtmlString,

    /// A mapping for any HTML attributes this start tag may have.
    ///
    /// Duplicate attributes are ignored after the first one as per WHATWG spec. Implement your own
    /// [crate::Emitter] to tweak this behavior.
    pub attributes: BTreeMap<HtmlString, Spanned<HtmlString, S>>,
    /// The span of the start tag. Includes exactly the `<p attr="value">`.
    pub span: Span<S>,
}

/// A HTML end/close tag, such as `</p>` or `</a>`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EndTag<S: SpanBound> {
    /// The ending tag's name, such as `"p"` or `"a"`.
    pub name: HtmlString,
    /// The span of the end tag. Includes exactly the `</p>`.
    pub span: Span<S>,
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
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Token<S: SpanBound> {
    /// A HTML start tag.
    StartTag(StartTag<S>),
    /// A HTML end tag.
    EndTag(EndTag<S>),
    /// A literal string.
    String(Spanned<HtmlString, S>),
    /// A HTML comment.
    Comment(Spanned<HtmlString, S>),
    /// A HTML doctype declaration.
    Doctype(Spanned<Doctype, S>),
    /// A HTML parsing error.
    ///
    /// Can be skipped over, the tokenizer is supposed to recover from the error and continues with
    /// more tokens afterward.
    Error(Spanned<Error, S>),
}
