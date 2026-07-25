//! Consume the parsed HTML through a set of typed, optional handler closures.
//!
//! [`EmitterBuilder`] is the successor to the old `CallbackEmitter`. Instead of routing every
//! parsing event through one closure matching on an event enum, each kind of event (tag open,
//! attribute, text, ...) gets its own handler slot, registered via `on_*` methods. Whether a slot
//! is filled is encoded in the emitter's type, so the tokenizer only maintains the buffers a
//! registered handler actually needs: an emitter built with only [`EmitterBuilder::on_attribute`]
//! does not pay for comment, doctype, or text buffering at all, and errors are not computed
//! unless [`EmitterBuilder::on_error`] is registered.
//!
//! Handlers receive the shared state as their first argument (`&mut St`) rather than capturing it
//! from the environment: since each event kind gets its own closure, two closures can't both hold
//! a mutable borrow of the same outer variable at once, so shared mutable state has to live in
//! `St`.
//!
//! ```
//! use html5gum::Tokenizer;
//! use html5gum::emitters::builder::sink;
//!
//! #[derive(Default)]
//! struct State {
//!     is_in_span: bool,
//!     pending: Option<Vec<u8>>,
//! }
//!
//! let emitter = sink(State::default())
//!     .on_tag_open(|st, name, _span| {
//!         st.is_in_span = name == b"span";
//!     })
//!     .on_text(|st, text, _span| {
//!         if st.is_in_span {
//!             st.pending = Some(text.to_vec());
//!         }
//!     })
//!     .on_end_tag(|st, _name, _span| {
//!         st.is_in_span = false;
//!     })
//!     .on_pop_token(|st| st.pending.take());
//!
//! let input = r#"<h1><span class=hello>Hello</span> world!</h1>"#;
//! let text_fragments = Tokenizer::new_with_emitter(input, emitter)
//!     .collect::<Result<Vec<_>, _>>()
//!     .unwrap();
//!
//! assert_eq!(text_fragments, vec![b"Hello".to_vec()]);
//! ```
//!
//! **Deliberate behavior change:** unlike the old `CallbackEmitter`, parsing errors are not
//! computed or delivered unless [`EmitterBuilder::on_error`] is registered.

// One type parameter per handler slot is the whole point of this design (see module docs); the
// resulting signatures are inherently long, not accidentally so.
#![allow(clippy::type_complexity)]

use std::convert::Infallible;

use crate::utils::trace_log;
use crate::{naive_next_state, Emitter, Error, Span, SpanBound, State};

mod sealed {
    pub trait Sealed {}
    impl Sealed for () {}
    impl<F> Sealed for super::Handler<F> {}
    impl<F> Sealed for super::ChunkHandler<F> {}
}

/// Wraps a user-provided closure so it can implement the sealed per-slot traits.
///
/// This indirection exists purely so that `()` (an empty slot) and `Handler<F>` (a filled slot)
/// are distinguishable to the compiler without relying on negative reasoning about `F`. You never
/// construct this type directly; the `on_*` methods on [`EmitterBuilder`] do it for you.
#[derive(Debug)]
#[doc(hidden)]
pub struct Handler<F>(F);

/// Like [`Handler`], but marks the text slot as streaming: the closure registered via
/// [`EmitterBuilder::on_text_chunk`] receives each chunk as it is produced, with no coalescing
/// buffer. Only usable in the text slot; the `on_text`/`on_text_chunk` methods construct it.
#[derive(Debug)]
#[doc(hidden)]
pub struct ChunkHandler<F>(F);

/// A borrowed view of a doctype token, passed to [`EmitterBuilder::on_doctype`].
#[derive(Debug, Clone, Copy)]
pub struct Doctype<'a> {
    /// The doctype's name. For HTML documents this is "html".
    pub name: &'a [u8],
    /// The doctype's public identifier, if any.
    pub public_identifier: Option<&'a [u8]>,
    /// The doctype's system identifier, if any.
    pub system_identifier: Option<&'a [u8]>,
    /// The ["force quirks"](https://html.spec.whatwg.org/#force-quirks-flag) flag.
    pub force_quirks: bool,
}

/// The spans of an attribute's name and value, passed to [`EmitterBuilder::on_attribute`].
#[derive(Debug, Clone, Copy)]
pub struct AttrSpans<S: SpanBound> {
    /// The span of the attribute's name.
    pub name: Span<S>,
    /// The span of the attribute's value. Empty (and equal to the end of the name span) if the
    /// attribute has no value, e.g. `<input disabled>`.
    pub value: Span<S>,
}

macro_rules! define_slot {
    ($trait_name:ident, $doc:literal, ($($arg_name:ident: $arg_ty:ty),* $(,)?)) => {
        #[doc = $doc]
        pub trait $trait_name<St, S: SpanBound>: sealed::Sealed {
            #[doc(hidden)]
            const PRESENT: bool;
            #[doc(hidden)]
            fn call(&mut self, state: &mut St, $($arg_name: $arg_ty),*);
        }

        impl<St, S: SpanBound> $trait_name<St, S> for () {
            const PRESENT: bool = false;
            #[inline]
            fn call(&mut self, _state: &mut St, $(_: $arg_ty),*) {}
        }

        impl<St, S: SpanBound, F> $trait_name<St, S> for Handler<F>
        where
            F: FnMut(&mut St, $($arg_ty),*),
        {
            const PRESENT: bool = true;
            #[inline]
            fn call(&mut self, state: &mut St, $($arg_name: $arg_ty),*) {
                (self.0)(state, $($arg_name),*)
            }
        }
    };
}

define_slot!(
    TagOpenSlot,
    "The sealed trait behind [`EmitterBuilder::on_tag_open`].",
    (name: &[u8], span: Span<S>)
);
define_slot!(
    AttributeSlot,
    "The sealed trait behind [`EmitterBuilder::on_attribute`].",
    (tag_name: &[u8], attr_name: &[u8], attr_value: &[u8], spans: AttrSpans<S>)
);
define_slot!(
    TagCloseSlot,
    "The sealed trait behind [`EmitterBuilder::on_tag_close`].",
    (tag_name: &[u8], self_closing: bool, span: Span<S>)
);
define_slot!(
    EndTagSlot,
    "The sealed trait behind [`EmitterBuilder::on_end_tag`].",
    (name: &[u8], span: Span<S>)
);
define_slot!(
    CommentSlot,
    "The sealed trait behind [`EmitterBuilder::on_comment`].",
    (value: &[u8], span: Span<S>)
);
define_slot!(
    DoctypeSlot,
    "The sealed trait behind [`EmitterBuilder::on_doctype`].",
    (doctype: Doctype<'_>, span: Span<S>)
);
define_slot!(
    ErrorSlot,
    "The sealed trait behind [`EmitterBuilder::on_error`]. Its presence *is* \
     [`Emitter::should_emit_errors`].",
    (error: Error, span: Span<S>)
);

/// The sealed trait behind [`EmitterBuilder::on_text`] and [`EmitterBuilder::on_text_chunk`].
///
/// Unlike the other slots, a filled text slot is one of two types: `Handler<F>` (from `on_text`,
/// coalescing all consecutive character tokens through a buffer) or `ChunkHandler<F>` (from
/// `on_text_chunk`, delivering each chunk straight from [`Emitter::emit_string`]). Since both
/// occupy the same slot, registering both on one builder is impossible.
pub trait TextSlot<St, S: SpanBound>: sealed::Sealed {
    #[doc(hidden)]
    const PRESENT: bool;
    /// Whether text is accumulated in `current_characters` and delivered in one coalesced call
    /// per run. If false while `PRESENT`, chunks bypass the buffer entirely.
    #[doc(hidden)]
    const COALESCE: bool;
    #[doc(hidden)]
    fn call(&mut self, state: &mut St, text: &[u8], span: Span<S>);
}

impl<St, S: SpanBound> TextSlot<St, S> for () {
    const PRESENT: bool = false;
    const COALESCE: bool = false;
    #[inline]
    fn call(&mut self, _state: &mut St, _: &[u8], _: Span<S>) {}
}

impl<St, S: SpanBound, F> TextSlot<St, S> for Handler<F>
where
    F: FnMut(&mut St, &[u8], Span<S>),
{
    const PRESENT: bool = true;
    const COALESCE: bool = true;
    #[inline]
    fn call(&mut self, state: &mut St, text: &[u8], span: Span<S>) {
        (self.0)(state, text, span)
    }
}

impl<St, S: SpanBound, F> TextSlot<St, S> for ChunkHandler<F>
where
    F: FnMut(&mut St, &[u8], Span<S>),
{
    const PRESENT: bool = true;
    const COALESCE: bool = false;
    #[inline]
    fn call(&mut self, state: &mut St, text: &[u8], span: Span<S>) {
        (self.0)(state, text, span)
    }
}

/// The sealed trait behind [`EmitterBuilder::on_pop_token`].
///
/// Unlike the other slots, this one determines [`Emitter::Token`]: if absent, `Token =
/// Infallible` and [`Emitter::pop_token`] always returns `None`.
pub trait PopSlot<St>: sealed::Sealed {
    #[doc(hidden)]
    type Token;
    #[doc(hidden)]
    fn call(&mut self, state: &mut St) -> Option<Self::Token>;
}

impl<St> PopSlot<St> for () {
    type Token = Infallible;
    #[inline]
    fn call(&mut self, _state: &mut St) -> Option<Infallible> {
        None
    }
}

impl<St, T, F> PopSlot<St> for Handler<F>
where
    F: FnMut(&mut St) -> Option<T>,
{
    type Token = T;
    #[inline]
    fn call(&mut self, state: &mut St) -> Option<T> {
        (self.0)(state)
    }
}

#[derive(Debug, Clone, Copy)]
enum CurrentTag {
    Start,
    End,
}

/// Fields maintained regardless of which handlers are registered, plus buffers that are only
/// touched when the corresponding slot is present. Factored out of [`EmitterBuilder`] so that
/// `on_*` methods can move it wholesale instead of destructuring/reconstructing every field by
/// hand each time a slot's type changes.
#[derive(Debug)]
struct Core<St, S: SpanBound> {
    state: St,
    naively_switch_states: bool,

    // Always maintained: required for correct tokenization regardless of which handlers are
    // registered (mirrors what a correct hand-written `Emitter` must track anyway).
    last_start_tag: Vec<u8>,
    current_tag_name: Vec<u8>,
    current_tag_type: Option<CurrentTag>,
    current_tag_self_closing: bool,
    current_tag_had_attributes: bool,
    current_taglike_span: S,
    position: S,

    // Gated on `OnText::PRESENT`. The buffer and its end bound are additionally gated on
    // `OnText::COALESCE`; in chunk mode (`on_text_chunk`) only `current_characters_start` is
    // maintained, tracking where the next chunk's span begins.
    current_characters: Vec<u8>,
    current_characters_start: S,
    current_characters_end: S,

    // Gated on `OnComment::PRESENT`.
    current_comment: Vec<u8>,

    // Gated on `OnAttribute::PRESENT`.
    current_attribute_name: Vec<u8>,
    current_attribute_value: Vec<u8>,
    current_attribute_name_start: S,
    current_attribute_name_end: S,
    current_attribute_value_start: S,
    current_attribute_value_end: S,

    // Gated on `OnDoctype::PRESENT`.
    doctype_name: Vec<u8>,
    doctype_has_public_identifier: bool,
    doctype_has_system_identifier: bool,
    doctype_public_identifier: Vec<u8>,
    doctype_system_identifier: Vec<u8>,
    doctype_force_quirks: bool,
}

impl<St, S: SpanBound> Core<St, S> {
    fn new(state: St) -> Self {
        Core {
            state,
            naively_switch_states: false,

            last_start_tag: Vec::new(),
            current_tag_name: Vec::new(),
            current_tag_type: None,
            current_tag_self_closing: false,
            current_tag_had_attributes: false,
            current_taglike_span: S::default(),
            position: S::default(),

            current_characters: Vec::new(),
            current_characters_start: S::default(),
            current_characters_end: S::default(),

            current_comment: Vec::new(),

            current_attribute_name: Vec::new(),
            current_attribute_value: Vec::new(),
            current_attribute_name_start: S::default(),
            current_attribute_name_end: S::default(),
            current_attribute_value_start: S::default(),
            current_attribute_value_end: S::default(),

            doctype_name: Vec::new(),
            doctype_has_public_identifier: false,
            doctype_has_system_identifier: false,
            doctype_public_identifier: Vec::new(),
            doctype_system_identifier: Vec::new(),
            doctype_force_quirks: false,
        }
    }
}

/// An [`Emitter`] built up from optional, typed handler closures.
///
/// Construct one with [`sink`] or [`spanned_sink`], then register handlers with the `on_*`
/// methods. See the [module docs](self) for a full example.
#[derive(Debug)]
pub struct EmitterBuilder<
    St,
    S: SpanBound = (),
    OnTagOpen = (),
    OnAttribute = (),
    OnTagClose = (),
    OnEndTag = (),
    OnText = (),
    OnComment = (),
    OnDoctype = (),
    OnError = (),
    OnPopToken = (),
> {
    core: Core<St, S>,
    on_tag_open: OnTagOpen,
    on_attribute: OnAttribute,
    on_tag_close: OnTagClose,
    on_end_tag: OnEndTag,
    on_text: OnText,
    on_comment: OnComment,
    on_doctype: OnDoctype,
    on_error: OnError,
    on_pop_token: OnPopToken,
}

/// Construct an [`EmitterBuilder`] that ignores spans (`S = ()`), wrapping the given user state.
#[must_use]
pub fn sink<St>(state: St) -> EmitterBuilder<St, ()> {
    spanned_sink(state)
}

/// Construct an [`EmitterBuilder`] tracking spans of type `S`, wrapping the given user state.
///
/// `S` is baked into every handler's signature, so it must be chosen up front and cannot change
/// after the first handler is registered. Use [`sink`] if you don't need spans.
#[must_use]
pub fn spanned_sink<S: SpanBound, St>(state: St) -> EmitterBuilder<St, S> {
    EmitterBuilder {
        core: Core::new(state),
        on_tag_open: (),
        on_attribute: (),
        on_tag_close: (),
        on_end_tag: (),
        on_text: (),
        on_comment: (),
        on_doctype: (),
        on_error: (),
        on_pop_token: (),
    }
}

impl<
        St,
        S: SpanBound,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        OnText,
        OnComment,
        OnDoctype,
        OnError,
        OnPopToken,
    >
    EmitterBuilder<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        OnText,
        OnComment,
        OnDoctype,
        OnError,
        OnPopToken,
    >
{
    /// Whether to use [`crate::naive_next_state`] to switch states automatically.
    ///
    /// The default is off.
    pub fn naively_switch_states(&mut self, yes: bool) {
        self.core.naively_switch_states = yes;
    }

    /// Get mutable access to the user state passed to [`sink`]/[`spanned_sink`].
    pub fn state_mut(&mut self) -> &mut St {
        &mut self.core.state
    }

    /// Register a handler for the start of a new start tag, e.g. the `<mytag` in `<mytag
    /// mykey=myvalue>`. Attributes have not yet been read.
    pub fn on_tag_open<F>(
        self,
        f: F,
    ) -> EmitterBuilder<
        St,
        S,
        Handler<F>,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        OnText,
        OnComment,
        OnDoctype,
        OnError,
        OnPopToken,
    >
    where
        F: FnMut(&mut St, &[u8], Span<S>),
    {
        EmitterBuilder {
            core: self.core,
            on_tag_open: Handler(f),
            on_attribute: self.on_attribute,
            on_tag_close: self.on_tag_close,
            on_end_tag: self.on_end_tag,
            on_text: self.on_text,
            on_comment: self.on_comment,
            on_doctype: self.on_doctype,
            on_error: self.on_error,
            on_pop_token: self.on_pop_token,
        }
    }

    /// Register a handler for an attribute, e.g. `mykey=myvalue` in `<mytag mykey=myvalue>`.
    /// Called once per attribute, with name and value delivered together, after both have been
    /// fully read. If the attribute has no value (e.g. `<input disabled>`), `attr_value` is
    /// empty. Duplicate attributes are reported as-is; deduplication is left to the handler.
    pub fn on_attribute<F>(
        self,
        f: F,
    ) -> EmitterBuilder<
        St,
        S,
        OnTagOpen,
        Handler<F>,
        OnTagClose,
        OnEndTag,
        OnText,
        OnComment,
        OnDoctype,
        OnError,
        OnPopToken,
    >
    where
        F: FnMut(&mut St, &[u8], &[u8], &[u8], AttrSpans<S>),
    {
        EmitterBuilder {
            core: self.core,
            on_tag_open: self.on_tag_open,
            on_attribute: Handler(f),
            on_tag_close: self.on_tag_close,
            on_end_tag: self.on_end_tag,
            on_text: self.on_text,
            on_comment: self.on_comment,
            on_doctype: self.on_doctype,
            on_error: self.on_error,
            on_pop_token: self.on_pop_token,
        }
    }

    /// Register a handler for the end of a start tag, e.g. the `>` in `<mytag mykey=myvalue>`.
    pub fn on_tag_close<F>(
        self,
        f: F,
    ) -> EmitterBuilder<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        Handler<F>,
        OnEndTag,
        OnText,
        OnComment,
        OnDoctype,
        OnError,
        OnPopToken,
    >
    where
        F: FnMut(&mut St, &[u8], bool, Span<S>),
    {
        EmitterBuilder {
            core: self.core,
            on_tag_open: self.on_tag_open,
            on_attribute: self.on_attribute,
            on_tag_close: Handler(f),
            on_end_tag: self.on_end_tag,
            on_text: self.on_text,
            on_comment: self.on_comment,
            on_doctype: self.on_doctype,
            on_error: self.on_error,
            on_pop_token: self.on_pop_token,
        }
    }

    /// Register a handler for an end tag, e.g. `</mytag>`.
    pub fn on_end_tag<F>(
        self,
        f: F,
    ) -> EmitterBuilder<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        Handler<F>,
        OnText,
        OnComment,
        OnDoctype,
        OnError,
        OnPopToken,
    >
    where
        F: FnMut(&mut St, &[u8], Span<S>),
    {
        EmitterBuilder {
            core: self.core,
            on_tag_open: self.on_tag_open,
            on_attribute: self.on_attribute,
            on_tag_close: self.on_tag_close,
            on_end_tag: Handler(f),
            on_text: self.on_text,
            on_comment: self.on_comment,
            on_doctype: self.on_doctype,
            on_error: self.on_error,
            on_pop_token: self.on_pop_token,
        }
    }

    /// Register a handler for a run of text between tags. It's guaranteed that all consecutive
    /// character tokens are coalesced into one call.
    ///
    /// The coalescing guarantee is paid for by buffering a copy of all text content. If your
    /// handler doesn't need whole runs, [`EmitterBuilder::on_text_chunk`] delivers text without
    /// buffering. The two methods fill the same slot, so only one of them can be registered.
    pub fn on_text<F>(
        self,
        f: F,
    ) -> EmitterBuilder<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        Handler<F>,
        OnComment,
        OnDoctype,
        OnError,
        OnPopToken,
    >
    where
        F: FnMut(&mut St, &[u8], Span<S>),
    {
        EmitterBuilder {
            core: self.core,
            on_tag_open: self.on_tag_open,
            on_attribute: self.on_attribute,
            on_tag_close: self.on_tag_close,
            on_end_tag: self.on_end_tag,
            on_text: Handler(f),
            on_comment: self.on_comment,
            on_doctype: self.on_doctype,
            on_error: self.on_error,
            on_pop_token: self.on_pop_token,
        }
    }

    /// Register a handler for a chunk of text, delivered as soon as the tokenizer produces it.
    ///
    /// Unlike [`EmitterBuilder::on_text`], no coalescing buffer is involved: text content is
    /// never copied, at the cost of **arbitrary chunk boundaries**. A single run of text arrives
    /// as any number of calls, and chunks may split words or the output of character references
    /// (e.g. `AT&amp;T` may arrive as `AT`, `&`, `T`). Only the concatenation of all chunks
    /// between two other events is meaningful; do not match on chunk contents directly.
    ///
    /// Each chunk's span covers just that chunk; concatenating a run's chunk spans yields the
    /// span that `on_text` would have reported for the run.
    ///
    /// This fills the same slot as `on_text`, so only one of the two can be registered.
    pub fn on_text_chunk<F>(
        self,
        f: F,
    ) -> EmitterBuilder<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        ChunkHandler<F>,
        OnComment,
        OnDoctype,
        OnError,
        OnPopToken,
    >
    where
        F: FnMut(&mut St, &[u8], Span<S>),
    {
        EmitterBuilder {
            core: self.core,
            on_tag_open: self.on_tag_open,
            on_attribute: self.on_attribute,
            on_tag_close: self.on_tag_close,
            on_end_tag: self.on_end_tag,
            on_text: ChunkHandler(f),
            on_comment: self.on_comment,
            on_doctype: self.on_doctype,
            on_error: self.on_error,
            on_pop_token: self.on_pop_token,
        }
    }

    /// Register a handler for a comment, e.g. `<!-- hello -->`.
    pub fn on_comment<F>(
        self,
        f: F,
    ) -> EmitterBuilder<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        OnText,
        Handler<F>,
        OnDoctype,
        OnError,
        OnPopToken,
    >
    where
        F: FnMut(&mut St, &[u8], Span<S>),
    {
        EmitterBuilder {
            core: self.core,
            on_tag_open: self.on_tag_open,
            on_attribute: self.on_attribute,
            on_tag_close: self.on_tag_close,
            on_end_tag: self.on_end_tag,
            on_text: self.on_text,
            on_comment: Handler(f),
            on_doctype: self.on_doctype,
            on_error: self.on_error,
            on_pop_token: self.on_pop_token,
        }
    }

    /// Register a handler for a doctype, e.g. `<!DOCTYPE html>`.
    pub fn on_doctype<F>(
        self,
        f: F,
    ) -> EmitterBuilder<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        OnText,
        OnComment,
        Handler<F>,
        OnError,
        OnPopToken,
    >
    where
        F: FnMut(&mut St, Doctype<'_>, Span<S>),
    {
        EmitterBuilder {
            core: self.core,
            on_tag_open: self.on_tag_open,
            on_attribute: self.on_attribute,
            on_tag_close: self.on_tag_close,
            on_end_tag: self.on_end_tag,
            on_text: self.on_text,
            on_comment: self.on_comment,
            on_doctype: Handler(f),
            on_error: self.on_error,
            on_pop_token: self.on_pop_token,
        }
    }

    /// Register a handler for parsing errors.
    ///
    /// Registering this handler is what makes [`Emitter::should_emit_errors`] return `true` for
    /// this emitter; if it's absent, error detection is skipped entirely.
    pub fn on_error<F>(
        self,
        f: F,
    ) -> EmitterBuilder<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        OnText,
        OnComment,
        OnDoctype,
        Handler<F>,
        OnPopToken,
    >
    where
        F: FnMut(&mut St, Error, Span<S>),
    {
        EmitterBuilder {
            core: self.core,
            on_tag_open: self.on_tag_open,
            on_attribute: self.on_attribute,
            on_tag_close: self.on_tag_close,
            on_end_tag: self.on_end_tag,
            on_text: self.on_text,
            on_comment: self.on_comment,
            on_doctype: self.on_doctype,
            on_error: Handler(f),
            on_pop_token: self.on_pop_token,
        }
    }

    /// Register a handler that produces tokens to be yielded from the [`crate::Tokenizer`]
    /// iterator. If absent, [`Emitter::Token`] is [`Infallible`] and [`Emitter::pop_token`]
    /// always returns `None`.
    pub fn on_pop_token<F, T>(
        self,
        f: F,
    ) -> EmitterBuilder<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        OnText,
        OnComment,
        OnDoctype,
        OnError,
        Handler<F>,
    >
    where
        F: FnMut(&mut St) -> Option<T>,
    {
        EmitterBuilder {
            core: self.core,
            on_tag_open: self.on_tag_open,
            on_attribute: self.on_attribute,
            on_tag_close: self.on_tag_close,
            on_end_tag: self.on_end_tag,
            on_text: self.on_text,
            on_comment: self.on_comment,
            on_doctype: self.on_doctype,
            on_error: self.on_error,
            on_pop_token: Handler(f),
        }
    }
}

impl<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        OnText,
        OnComment,
        OnDoctype,
        OnError,
        OnPopToken,
    >
    EmitterBuilder<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        OnText,
        OnComment,
        OnDoctype,
        OnError,
        OnPopToken,
    >
where
    S: SpanBound,
    OnTagOpen: TagOpenSlot<St, S>,
    OnAttribute: AttributeSlot<St, S>,
    OnTagClose: TagCloseSlot<St, S>,
    OnEndTag: EndTagSlot<St, S>,
    OnText: TextSlot<St, S>,
    OnComment: CommentSlot<St, S>,
    OnDoctype: DoctypeSlot<St, S>,
    OnError: ErrorSlot<St, S>,
    OnPopToken: PopSlot<St>,
{
    fn flush_tag_open(&mut self) {
        self.flush_current_characters();

        if matches!(self.core.current_tag_type, Some(CurrentTag::Start))
            && !self.core.current_tag_name.is_empty()
        {
            if OnTagOpen::PRESENT {
                let span = Span {
                    start: self.core.current_taglike_span,
                    end: self.core.position.offset(-1),
                };
                self.on_tag_open
                    .call(&mut self.core.state, &self.core.current_tag_name, span);
            }

            self.core.last_start_tag.clear();
            std::mem::swap(
                &mut self.core.last_start_tag,
                &mut self.core.current_tag_name,
            );
        }
    }

    fn flush_attribute(&mut self) {
        self.flush_tag_open();

        if OnAttribute::PRESENT && !self.core.current_attribute_name.is_empty() {
            // `flush_tag_open` above moves the tag name into `last_start_tag` for start tags
            // (clearing `current_tag_name`); for end tags (attributes there are a spec
            // violation, reported separately as `EndTagWithAttributes`) no swap happens and the
            // name is still in `current_tag_name`.
            let tag_name: &[u8] = match self.core.current_tag_type {
                Some(CurrentTag::Start) => &self.core.last_start_tag,
                _ => &self.core.current_tag_name,
            };
            let spans = AttrSpans {
                name: Span {
                    start: self.core.current_attribute_name_start,
                    end: self.core.current_attribute_name_end,
                },
                value: Span {
                    start: self.core.current_attribute_value_start,
                    end: self.core.current_attribute_value_end,
                },
            };
            self.on_attribute.call(
                &mut self.core.state,
                tag_name,
                &self.core.current_attribute_name,
                &self.core.current_attribute_value,
                spans,
            );
            self.core.current_attribute_name.clear();
            self.core.current_attribute_value.clear();
        }
    }

    fn flush_current_characters(&mut self) {
        // In chunk mode (`PRESENT` but not `COALESCE`) nothing is buffered: every chunk was
        // already delivered from `emit_string`, in order, before whatever event triggers this
        // flush.
        if !OnText::COALESCE || self.core.current_characters.is_empty() {
            return;
        }

        let span = Span {
            start: self.core.current_characters_start,
            end: self.core.current_characters_end,
        };
        self.on_text
            .call(&mut self.core.state, &self.core.current_characters, span);
        self.core.current_characters.clear();
    }
}

impl<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        OnText,
        OnComment,
        OnDoctype,
        OnError,
        OnPopToken,
    > Emitter
    for EmitterBuilder<
        St,
        S,
        OnTagOpen,
        OnAttribute,
        OnTagClose,
        OnEndTag,
        OnText,
        OnComment,
        OnDoctype,
        OnError,
        OnPopToken,
    >
where
    S: SpanBound,
    OnTagOpen: TagOpenSlot<St, S>,
    OnAttribute: AttributeSlot<St, S>,
    OnTagClose: TagCloseSlot<St, S>,
    OnEndTag: EndTagSlot<St, S>,
    OnText: TextSlot<St, S>,
    OnComment: CommentSlot<St, S>,
    OnDoctype: DoctypeSlot<St, S>,
    OnError: ErrorSlot<St, S>,
    OnPopToken: PopSlot<St>,
{
    type Token = <OnPopToken as PopSlot<St>>::Token;

    #[inline]
    fn move_position(&mut self, offset: isize) {
        self.core.position = self.core.position.offset(offset);
        trace_log!(
            "builder: move_position, offset={}, now={:?}",
            offset,
            self.core.position
        );
    }

    #[inline]
    fn should_emit_errors(&mut self) -> bool {
        OnError::PRESENT
    }

    fn set_last_start_tag(&mut self, last_start_tag: Option<&[u8]>) {
        self.core.last_start_tag.clear();
        self.core
            .last_start_tag
            .extend(last_start_tag.unwrap_or_default());
    }

    fn emit_eof(&mut self) {
        self.flush_current_characters();
    }

    fn emit_error(&mut self, error: Error) {
        if OnError::PRESENT {
            let span = Span {
                start: self.core.position,
                end: self.core.position,
            };
            self.on_error.call(&mut self.core.state, error, span);
        }
    }

    fn pop_token(&mut self) -> Option<Self::Token> {
        self.on_pop_token.call(&mut self.core.state)
    }

    fn init_string(&mut self) {
        // In chunk mode `current_characters` is never written to, so this unconditionally
        // (re)starts the pending chunk span.
        if OnText::PRESENT && self.core.current_characters.is_empty() {
            self.core.current_characters_start = self.core.position;
        }
    }

    fn emit_string(&mut self, s: &[u8]) {
        if !OnText::PRESENT {
            return;
        }
        if OnText::COALESCE {
            self.core.current_characters_end = self.core.position;
            self.core.current_characters.extend(s);
        } else {
            // The span start is wherever the previous chunk (or `init_string`) left off; the
            // machine doesn't call `init_string` between chunks of one run.
            let span = Span {
                start: self.core.current_characters_start,
                end: self.core.position,
            };
            self.core.current_characters_start = self.core.position;
            if !s.is_empty() {
                self.on_text.call(&mut self.core.state, s, span);
            }
        }
    }

    fn init_start_tag(&mut self) {
        self.core.current_tag_name.clear();
        self.core.current_tag_type = Some(CurrentTag::Start);
        self.core.current_tag_self_closing = false;
    }

    fn init_end_tag(&mut self) {
        self.core.current_tag_name.clear();
        self.core.current_tag_type = Some(CurrentTag::End);
        self.core.current_tag_had_attributes = false;
    }

    fn init_comment(&mut self) {
        self.flush_current_characters();
        if OnComment::PRESENT {
            self.core.current_comment.clear();
        }
    }

    fn emit_current_tag(&mut self) -> Option<State> {
        self.flush_attribute();

        match self.core.current_tag_type {
            Some(CurrentTag::Start) => {
                self.flush_tag_open();
                if OnTagClose::PRESENT {
                    let s = self.core.position;
                    self.on_tag_close.call(
                        &mut self.core.state,
                        &self.core.last_start_tag,
                        self.core.current_tag_self_closing,
                        Span {
                            start: s.offset(-1),
                            end: s,
                        },
                    );
                }
            }
            Some(CurrentTag::End) => {
                if self.core.current_tag_had_attributes {
                    self.emit_error(Error::EndTagWithAttributes);
                }
                self.core.last_start_tag.clear();
                if OnEndTag::PRESENT {
                    let span = Span {
                        start: self.core.current_taglike_span,
                        end: self.core.position,
                    };
                    self.on_end_tag
                        .call(&mut self.core.state, &self.core.current_tag_name, span);
                }
            }
            None => {}
        }

        let next_state = if self.core.naively_switch_states {
            naive_next_state(&self.core.last_start_tag)
        } else {
            None
        };

        if next_state.is_some() {
            self.init_string();
        }

        next_state
    }

    fn emit_current_comment(&mut self) {
        if OnComment::PRESENT {
            let span = Span {
                start: self.core.current_taglike_span,
                end: self.core.position,
            };
            self.on_comment
                .call(&mut self.core.state, &self.core.current_comment, span);
            self.core.current_comment.clear();
        }
    }

    fn emit_current_doctype(&mut self) {
        if OnDoctype::PRESENT {
            let doctype = Doctype {
                name: &self.core.doctype_name,
                public_identifier: if self.core.doctype_has_public_identifier {
                    Some(&self.core.doctype_public_identifier)
                } else {
                    None
                },
                system_identifier: if self.core.doctype_has_system_identifier {
                    Some(&self.core.doctype_system_identifier)
                } else {
                    None
                },
                force_quirks: self.core.doctype_force_quirks,
            };
            let span = Span {
                start: self.core.current_taglike_span,
                end: self.core.position,
            };
            self.on_doctype.call(&mut self.core.state, doctype, span);
        }
    }

    fn set_self_closing(&mut self) {
        trace_log!("builder: set_self_closing");
        if matches!(self.core.current_tag_type, Some(CurrentTag::End)) {
            self.emit_error(Error::EndTagWithTrailingSolidus);
        } else {
            self.core.current_tag_self_closing = true;
        }
    }

    fn set_force_quirks(&mut self) {
        if OnDoctype::PRESENT {
            self.core.doctype_force_quirks = true;
        }
    }

    fn push_tag_name(&mut self, s: &[u8]) {
        self.core.current_tag_name.extend(s);
    }

    fn push_comment(&mut self, s: &[u8]) {
        if OnComment::PRESENT {
            self.core.current_comment.extend(s);
        }
    }

    fn push_doctype_name(&mut self, s: &[u8]) {
        if OnDoctype::PRESENT {
            self.core.doctype_name.extend(s);
        }
    }

    fn init_doctype(&mut self) {
        self.flush_current_characters();
        if OnDoctype::PRESENT {
            self.core.doctype_name.clear();
            self.core.doctype_has_public_identifier = false;
            self.core.doctype_has_system_identifier = false;
            self.core.doctype_public_identifier.clear();
            self.core.doctype_system_identifier.clear();
            self.core.doctype_force_quirks = false;
        }
    }

    fn init_attribute(&mut self) {
        self.flush_attribute();
        self.core.current_tag_had_attributes = true;
        if OnAttribute::PRESENT {
            self.core.current_attribute_name_start = self.core.position.offset(-1);
            // Reset rather than leave stale: if this attribute turns out to have no value
            // (`init_attribute_value`/`push_attribute_value` never called, e.g. `<input
            // disabled>`), `spans.value` in `on_attribute` must not leak the previous
            // attribute's span.
            self.core.current_attribute_value_start = self.core.position.offset(-1);
            self.core.current_attribute_value_end = self.core.position.offset(-1);
        }
    }

    fn push_attribute_name(&mut self, s: &[u8]) {
        if OnAttribute::PRESENT {
            self.core.current_attribute_name.extend(s);
            self.core.current_attribute_name_end = self.core.position;
        }
    }

    fn init_attribute_value(&mut self) {
        if OnAttribute::PRESENT {
            self.core.current_attribute_value_start = self.core.position;
        }
    }

    fn push_attribute_value(&mut self, s: &[u8]) {
        if OnAttribute::PRESENT {
            self.core.current_attribute_value.extend(s);
            self.core.current_attribute_value_end = self.core.position;
        }
    }

    fn set_doctype_public_identifier(&mut self, value: &[u8]) {
        if OnDoctype::PRESENT {
            self.core.doctype_has_public_identifier = true;
            self.core.doctype_public_identifier.clear();
            self.core.doctype_public_identifier.extend(value);
        }
    }

    fn set_doctype_system_identifier(&mut self, value: &[u8]) {
        if OnDoctype::PRESENT {
            self.core.doctype_has_system_identifier = true;
            self.core.doctype_system_identifier.clear();
            self.core.doctype_system_identifier.extend(value);
        }
    }

    fn push_doctype_public_identifier(&mut self, value: &[u8]) {
        if OnDoctype::PRESENT {
            self.core.doctype_public_identifier.extend(value);
        }
    }

    fn push_doctype_system_identifier(&mut self, value: &[u8]) {
        if OnDoctype::PRESENT {
            self.core.doctype_system_identifier.extend(value);
        }
    }

    fn start_open_tag(&mut self) {
        self.core.current_taglike_span = self.core.position.offset(-1);
    }

    fn current_is_appropriate_end_tag_token(&mut self) -> bool {
        if self.core.last_start_tag.is_empty() {
            return false;
        }

        if !matches!(self.core.current_tag_type, Some(CurrentTag::End)) {
            return false;
        }

        self.core.last_start_tag == self.core.current_tag_name
    }
}

#[cfg(test)]
mod tests {
    use super::sink;
    use crate::Tokenizer;

    #[test]
    fn round_trip() {
        // Shared mutable state has to live in `St`, since each event kind gets its own closure
        // and closures can't all capture the same outer variable mutably at once.
        let emitter = sink(Vec::<u8>::new())
            .on_tag_open(|rt, name, _span| {
                rt.push(b'<');
                rt.extend(name);
            })
            .on_attribute(|rt, _tag_name, name, value, _spans| {
                rt.push(b' ');
                rt.extend(name);
                rt.push(b'=');
                rt.push(b'"');
                rt.extend(value);
                rt.push(b'"');
            })
            .on_tag_close(|rt, _tag_name, self_closing, _span| {
                if self_closing {
                    rt.push(b'/');
                }
                rt.push(b'>');
            })
            .on_end_tag(|rt, name, _span| {
                rt.extend(b"</");
                rt.extend(name);
                rt.push(b'>');
            })
            .on_text(|rt, text, _span| {
                rt.extend(text);
            })
            .on_comment(|rt, value, _span| {
                rt.extend(b"<!--");
                rt.extend(value);
                rt.extend(b"-->");
            });

        let source = " <!-- a --> <h1>Hello</h1> world <a href=\"foo\" title=\"baz\">bar</a>";
        let mut tokenizer = Tokenizer::new_with_emitter(source, emitter);
        for result in &mut tokenizer {
            result.unwrap();
        }
        let rt = tokenizer.emitter.state_mut();
        assert_eq!(source.as_bytes(), rt, "{} != {}", source, rt.escape_ascii());
    }

    #[test]
    fn coalesced_text_is_one_call_per_run() {
        // `on_text` delivers each run of consecutive character tokens as exactly one call, even
        // though the tokenizer produces the run in several chunks (here: around the character
        // reference).
        let emitter = sink(Vec::<Vec<u8>>::new()).on_text(|runs, text, _span| {
            runs.push(text.to_vec());
        });

        let mut tokenizer = Tokenizer::new_with_emitter("AT&amp;T <b>bold</b> end", emitter);
        for result in &mut tokenizer {
            result.unwrap();
        }
        assert_eq!(
            tokenizer.emitter.state_mut(),
            &vec![b"AT&T ".to_vec(), b"bold".to_vec(), b" end".to_vec()]
        );
    }

    #[test]
    fn text_chunk_round_trip() {
        // `on_text_chunk` may deliver a run as arbitrarily many chunks (splitting around
        // character references, or per-character in raw text), but interleaved with the tag
        // events, the chunks must reproduce exactly what `on_text` would have delivered.
        fn tag_open(rt: &mut Vec<u8>, name: &[u8], _span: crate::Span<()>) {
            rt.push(b'<');
            rt.extend(name);
        }
        fn tag_close(rt: &mut Vec<u8>, _name: &[u8], _self_closing: bool, _span: crate::Span<()>) {
            rt.push(b'>');
        }
        fn end_tag(rt: &mut Vec<u8>, name: &[u8], _span: crate::Span<()>) {
            rt.extend(b"</");
            rt.extend(name);
            rt.push(b'>');
        }
        fn text(rt: &mut Vec<u8>, text: &[u8], _span: crate::Span<()>) {
            rt.extend(text);
        }

        let source = "x &amp;&amp; y <h1>AT&amp;T</h1><script>1 < 2 &amp; 3</script> tail&lt;";

        let mut coalesced = sink(Vec::<u8>::new())
            .on_tag_open(tag_open)
            .on_tag_close(tag_close)
            .on_end_tag(end_tag)
            .on_text(text);
        coalesced.naively_switch_states(true);
        let mut chunked = sink(Vec::<u8>::new())
            .on_tag_open(tag_open)
            .on_tag_close(tag_close)
            .on_end_tag(end_tag)
            .on_text_chunk(text);
        chunked.naively_switch_states(true);

        let mut tokenizer = Tokenizer::new_with_emitter(source, coalesced);
        for result in &mut tokenizer {
            result.unwrap();
        }
        let coalesced_output = std::mem::take(tokenizer.emitter.state_mut());

        let mut tokenizer = Tokenizer::new_with_emitter(source, chunked);
        for result in &mut tokenizer {
            result.unwrap();
        }
        let chunked_output = std::mem::take(tokenizer.emitter.state_mut());

        // character references outside of raw text are resolved, everything else round-trips
        let expected: &[u8] = b"x && y <h1>AT&T</h1><script>1 < 2 &amp; 3</script> tail<";
        assert_eq!(
            coalesced_output,
            expected,
            "{}",
            coalesced_output.escape_ascii()
        );
        assert_eq!(chunked_output, expected, "{}", chunked_output.escape_ascii());
    }

    #[test]
    fn text_chunk_spans_tile_the_run() {
        // Consecutive chunk spans must be contiguous: each chunk starts where the previous one
        // ended, and together they cover the same range `on_text` would have reported.
        let emitter = crate::emitters::builder::spanned_sink::<usize, _>(Vec::new())
            .on_text_chunk(|chunks: &mut Vec<(Vec<u8>, crate::Span<usize>)>, text, span| {
                chunks.push((text.to_vec(), span));
            });

        let source = "AT&amp;T rocks";
        let mut tokenizer = Tokenizer::new_with_emitter(source, emitter);
        for result in &mut tokenizer {
            result.unwrap();
        }
        let chunks = std::mem::take(tokenizer.emitter.state_mut());

        let mut concatenated = Vec::<u8>::new();
        let mut prev_end = 0;
        for (text, span) in &chunks {
            assert_eq!(span.start, prev_end);
            prev_end = span.end;
            concatenated.extend(text);
        }
        assert_eq!(concatenated, b"AT&T rocks");
        assert_eq!(chunks.first().unwrap().1.start, 0);
        assert_eq!(chunks.last().unwrap().1.end, source.len());
    }

    #[test]
    fn empty_slots_still_tokenize_correctly() {
        // An emitter with no handlers registered at all must still drive the tokenizer's
        // internal state machine correctly: appropriate-end-tag-token detection and
        // `naively_switch_states` (rawtext/rcdata/script handling) don't depend on any handler
        // being present.
        let mut emitter = sink(());
        emitter.naively_switch_states(true);

        // If `<script>` content were tokenized as regular markup instead of raw text, this would
        // produce a stray `<div>` open tag event, which we can't observe directly here (no
        // handlers), but the tokenizer would still panic/misbehave on malformed appropriate
        // end-tag handling if internal state tracking were broken. This should simply not panic
        // and consume all input.
        let source = "<script><div></script><style>a { color: red }</style>after";
        Tokenizer::new_with_emitter(source, emitter)
            .finish()
            .unwrap();
    }
}
