//! The default emitter is what powers the simple SAX-like API that you see in the README.
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::mem::take;

use crate::{Error, HtmlString, Span, SpanBound, Spanned};

use crate::emitters::callback::{Callback, CallbackEmitter, CallbackEvent};

use super::{Emitter, ForwardingEmitter};

#[derive(Debug, Default)]
struct OurCallback<S: SpanBound> {
    tag_name: Vec<u8>,
    tag_start_span: S,
    attribute_name: Spanned<HtmlString, S>,
    attribute_map: BTreeMap<HtmlString, Spanned<HtmlString, S>>,
}

impl<S: SpanBound> Callback<Token<S>, S> for OurCallback<S> {
    fn handle_event(&mut self, event: CallbackEvent<'_>, span: Span<S>) -> Option<Token<S>> {
        crate::utils::trace_log!("event: {:?}, span={:?}", event, span);
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
pub struct DefaultEmitter<S: SpanBound = ()> {
    inner: CallbackEmitter<OurCallback<S>, Token<S>, S>,
}

impl Default for DefaultEmitter<()> {
    fn default() -> Self {
        Self {
            inner: Default::default(),
        }
    }
}

impl<S: SpanBound> DefaultEmitter<S> {
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

impl<S: SpanBound> ForwardingEmitter for DefaultEmitter<S> {
    type Token = Token<S>;

    fn inner(&mut self) -> &mut impl Emitter<Token = Self::Token> {
        &mut self.inner
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
pub enum Token<S: SpanBound = ()> {
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

impl<S: SpanBound> Token<S> {
    /// Map the span type to another span type using the provided function.
    pub fn map_span<T: SpanBound>(self, mut f: impl FnMut(Span<S>) -> Span<T>) -> Token<T> {
        match self {
            Token::StartTag(tag) => Token::StartTag(StartTag {
                self_closing: tag.self_closing,
                name: tag.name,
                attributes: tag
                    .attributes
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            k,
                            Spanned {
                                value: v.value,
                                span: f(v.span),
                            },
                        )
                    })
                    .collect(),
                span: f(tag.span),
            }),
            Token::EndTag(tag) => Token::EndTag(EndTag {
                name: tag.name,
                span: f(tag.span),
            }),
            Token::String(s) => Token::String(Spanned {
                value: s.value,
                span: f(s.span),
            }),
            Token::Comment(c) => Token::Comment(Spanned {
                value: c.value,
                span: f(c.span),
            }),
            Token::Doctype(d) => Token::Doctype(Spanned {
                value: d.value,
                span: f(d.span),
            }),
            Token::Error(e) => Token::Error(Spanned {
                value: e.value,
                span: f(e.span),
            }),
        }
    }
}
