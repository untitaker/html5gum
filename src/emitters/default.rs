//! The default emitter is what powers the simple SAX-like API that you see in the README.
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, VecDeque};

use crate::emitters::builder::{
    spanned_sink, AttrSpans, Doctype as BuilderDoctype, EmitterBuilder, Handler,
};
use crate::{Error, HtmlString, Span, SpanBound, Spanned};

use super::{Emitter, ForwardingEmitter};

#[derive(Debug)]
struct State<S: SpanBound> {
    tag_start_span: S,
    attribute_map: BTreeMap<HtmlString, Spanned<HtmlString, S>>,
    tokens: VecDeque<Token<S>>,
}

impl<S: SpanBound> Default for State<S> {
    fn default() -> Self {
        State {
            tag_start_span: S::default(),
            attribute_map: BTreeMap::new(),
            tokens: VecDeque::new(),
        }
    }
}

fn on_tag_open<S: SpanBound>(state: &mut State<S>, _name: &[u8], span: Span<S>) {
    state.tag_start_span = span.start;
}

fn on_attribute<S: SpanBound>(
    state: &mut State<S>,
    _tag_name: &[u8],
    name: &[u8],
    value: &[u8],
    spans: AttrSpans<S>,
) {
    match state.attribute_map.entry(name.to_owned().into()) {
        Entry::Occupied(_) => {
            state.tokens.push_back(Token::Error(Spanned {
                value: Error::DuplicateAttribute,
                span: spans.name,
            }));
        }
        Entry::Vacant(vacant) => {
            // If no value was ever read (boolean attribute, e.g. `<input disabled>`), the
            // reported span never advances past the attribute name. Otherwise it extends one
            // past the value's own end, to account for the closing quote.
            let end = if value.is_empty() {
                spans.name.end
            } else {
                spans.value.end.offset(1)
            };
            vacant.insert(Spanned {
                value: value.to_owned().into(),
                span: Span {
                    start: spans.name.start,
                    end,
                },
            });
        }
    }
}

fn on_tag_close<S: SpanBound>(
    state: &mut State<S>,
    tag_name: &[u8],
    self_closing: bool,
    span: Span<S>,
) {
    state.tokens.push_back(Token::StartTag(StartTag {
        self_closing,
        name: tag_name.to_owned().into(),
        span: Span {
            start: state.tag_start_span,
            end: span.end,
        },
        attributes: std::mem::take(&mut state.attribute_map),
    }));
}

fn on_end_tag<S: SpanBound>(state: &mut State<S>, name: &[u8], span: Span<S>) {
    state.attribute_map.clear();
    state.tokens.push_back(Token::EndTag(EndTag {
        name: name.to_owned().into(),
        span,
    }));
}

fn on_text<S: SpanBound>(state: &mut State<S>, text: &[u8], span: Span<S>) {
    state.tokens.push_back(Token::String(Spanned {
        value: text.to_owned().into(),
        span,
    }));
}

fn on_comment<S: SpanBound>(state: &mut State<S>, value: &[u8], span: Span<S>) {
    state.tokens.push_back(Token::Comment(Spanned {
        value: value.to_owned().into(),
        span,
    }));
}

fn on_doctype<S: SpanBound>(state: &mut State<S>, doctype: BuilderDoctype<'_>, span: Span<S>) {
    state.tokens.push_back(Token::Doctype(Spanned {
        value: Doctype {
            force_quirks: doctype.force_quirks,
            name: doctype.name.to_owned().into(),
            public_identifier: doctype.public_identifier.map(|x| x.to_owned().into()),
            system_identifier: doctype.system_identifier.map(|x| x.to_owned().into()),
        },
        span,
    }));
}

fn on_error<S: SpanBound>(state: &mut State<S>, error: Error, span: Span<S>) {
    state
        .tokens
        .push_back(Token::Error(Spanned { value: error, span }));
}

fn on_pop_token<S: SpanBound>(state: &mut State<S>) -> Option<Token<S>> {
    state.tokens.pop_front()
}

// None of the handlers above capture any environment (all state lives in `State<S>`), so they
// coerce to plain function pointers. That keeps this type alias nameable, unlike the closures a
// direct `EmitterBuilder` user would normally register.
type Inner<S> = EmitterBuilder<
    State<S>,
    S,
    Handler<fn(&mut State<S>, &[u8], Span<S>)>,
    Handler<fn(&mut State<S>, &[u8], &[u8], &[u8], AttrSpans<S>)>,
    Handler<fn(&mut State<S>, &[u8], bool, Span<S>)>,
    Handler<fn(&mut State<S>, &[u8], Span<S>)>,
    Handler<fn(&mut State<S>, &[u8], Span<S>)>,
    Handler<fn(&mut State<S>, &[u8], Span<S>)>,
    Handler<fn(&mut State<S>, BuilderDoctype<'_>, Span<S>)>,
    Handler<fn(&mut State<S>, Error, Span<S>)>,
    Handler<fn(&mut State<S>) -> Option<Token<S>>>,
>;

fn build_inner<S: SpanBound>() -> Inner<S> {
    spanned_sink(State::default())
        .on_tag_open(on_tag_open::<S> as fn(&mut State<S>, &[u8], Span<S>))
        .on_attribute(on_attribute::<S> as fn(&mut State<S>, &[u8], &[u8], &[u8], AttrSpans<S>))
        .on_tag_close(on_tag_close::<S> as fn(&mut State<S>, &[u8], bool, Span<S>))
        .on_end_tag(on_end_tag::<S> as fn(&mut State<S>, &[u8], Span<S>))
        .on_text(on_text::<S> as fn(&mut State<S>, &[u8], Span<S>))
        .on_comment(on_comment::<S> as fn(&mut State<S>, &[u8], Span<S>))
        .on_doctype(on_doctype::<S> as fn(&mut State<S>, BuilderDoctype<'_>, Span<S>))
        .on_error(on_error::<S> as fn(&mut State<S>, Error, Span<S>))
        .on_pop_token(on_pop_token::<S> as fn(&mut State<S>) -> Option<Token<S>>)
}

/// This is the emitter you implicitly use with [crate::Tokenizer::new]. Refer to the [crate
/// docs](crate) for how usage looks like.
#[derive(Debug)]
pub struct DefaultEmitter<S: SpanBound = ()> {
    inner: Inner<S>,
}

impl Default for DefaultEmitter<()> {
    fn default() -> Self {
        Self {
            inner: build_inner(),
        }
    }
}

impl<S: SpanBound> DefaultEmitter<S> {
    /// Create a new [`DefaultEmitter`] for a certain [`Span`]type which you can pass to
    /// [`crate::Tokenizer::new_with_emitter`].
    #[must_use]
    pub fn new_with_span() -> Self {
        Self {
            inner: build_inner(),
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
