use crate::{Error, State};

/// An emitter is an object providing methods to the tokenizer to produce tokens.
///
/// Domain-specific applications of the HTML tokenizer can manually implement this trait to
/// customize per-token allocations, or avoid them altogether.
///
/// An emitter is assumed to have these internal states:
///
/// * _last start tag_: The most recently emitted start tag's name
/// * _current token_: Can be a tag, doctype or comment token. There's only one current token.
/// * _current attribute_: The currently processed HTML attribute, consisting of two strings for name and value.
///
/// The following methods are describing what kind of behavior the WHATWG spec expects, but that
/// doesn't mean you need to follow it. For example:
///
/// * If your usage of the tokenizer will ignore all errors, none of the error handling and
///   validation requirements apply to you. You can implement `emit_error` as noop and omit all
///   checks that would emit errors.
///
/// * If you don't care about attributes at all, you can make all related methods a noop.
///
/// The state machine needs to have a functional implementation of
/// `current_is_appropriate_end_tag_token` to do correct transitions, however.
pub trait Emitter<R> {
    /// The token type emitted by this emitter. This controls what type of values the [`crate::Tokenizer`]
    /// yields when used as an iterator.
    type Token;

    /// Set the name of the _last start tag_.
    ///
    /// This is primarily for testing purposes. This is *not* supposed to override the tag name of
    /// the current tag.
    fn set_last_start_tag(&mut self, last_start_tag: Option<&[u8]>, reader: &R);

    /// The state machine has reached the end of the file. It will soon call `pop_token` for the
    /// last time.
    fn emit_eof(&mut self, reader: &R);

    /// A (probably recoverable) parsing error has occured.
    fn emit_error(&mut self, error: Error, reader: &R);

    /// Whether this emitter cares about errors at all.
    ///
    /// If your implementation of `emit_error` is a noop, you can override this function to return
    /// `false` and decorate it with `#[inline]` to aid the compiler in optimizing out more dead
    /// code.
    ///
    /// This method should return the same value at all times. Returning different values on each
    /// call might cause bogus errors to be emitted.
    #[inline]
    #[must_use]
    fn should_emit_errors(&mut self) -> bool {
        // should_emit_errors takes self so that users can implement it as a runtime option in
        // their program. It takes &mut for no particular reason other than _potential_
        // convenience, and just because we can provide that guarantee to the emitter.
        true
    }

    /// After every state change, the tokenizer calls this method to retrieve a new token that can
    /// be returned via the tokenizer's iterator interface.
    fn pop_token(&mut self, reader: &R) -> Option<Self::Token>;

    /// Emit a bunch of plain characters as character tokens.
    fn emit_string(&mut self, c: &[u8], reader: &R);

    /// Set the _current token_ to a start tag.
    fn init_start_tag(&mut self, reader: &R);

    /// Set the _current token_ to an end tag.
    fn init_end_tag(&mut self, reader: &R);

    /// Set the _current token_ to a comment.
    fn init_comment(&mut self, reader: &R);

    /// Emit the _current token_, assuming it is a tag.
    ///
    /// Also get the current attribute and append it to the to-be-emitted tag. See docstring for
    /// [`Emitter::init_attribute`] for how duplicates should be handled.
    ///
    /// If a start tag is emitted, update the _last start tag_.
    ///
    /// If the current token is not a start/end tag, this method may panic.
    ///
    /// The return value is used to switch the tokenizer to a new state. Used in tree building.
    ///
    /// If this method always returns `None`, states are never switched, which leads to artifacts
    /// like contents of `<script>` tags being incorrectly interpreted as HTML.
    ///
    /// It's not possible to implement this method correctly in line with the spec without
    /// implementing a full-blown tree builder as per [tree
    /// construction](https://html.spec.whatwg.org/#tree-construction), which this crate does not
    /// offer.
    ///
    /// You can approximate correct behavior using [`naive_next_state`], but the caveats of doing
    /// so are not well-understood.
    ///
    /// See the `tokenize_with_state_switches` cargo example for a practical example where this
    /// matters.
    #[must_use]
    fn emit_current_tag(&mut self, reader: &R) -> Option<State>;

    /// Emit the _current token_, assuming it is a comment.
    ///
    /// If the current token is not a comment, this method may panic.
    fn emit_current_comment(&mut self, reader: &R);

    /// Emit the _current token_, assuming it is a doctype.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn emit_current_doctype(&mut self, reader: &R);

    /// Assuming the _current token_ is a start tag, set the self-closing flag.
    ///
    /// If the current token is not a start or end tag, this method may panic.
    ///
    /// If the current token is an end tag, the emitter should emit the
    /// [`crate::Error::EndTagWithTrailingSolidus`] error.
    fn set_self_closing(&mut self, reader: &R);

    /// Assuming the _current token_ is a doctype, set its "force quirks" flag to true.
    ///
    /// If the current token is not a doctype, this method pay panic.
    fn set_force_quirks(&mut self, reader: &R);

    /// Assuming the _current token_ is a start/end tag, append a string to the current tag's name.
    ///
    /// If the current token is not a start or end tag, this method may panic.
    fn push_tag_name(&mut self, s: &[u8], reader: &R);

    /// Assuming the _current token_ is a comment, append a string to the comment's contents.
    ///
    /// If the current token is not a comment, this method may panic.
    fn push_comment(&mut self, s: &[u8], reader: &R);

    /// Assuming the _current token_ is a doctype, append a string to the doctype's name.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn push_doctype_name(&mut self, s: &[u8], reader: &R);

    /// Set the _current token_ to a new doctype token:
    ///
    /// * the name should be empty
    /// * the "public identifier" should be null (different from empty)
    /// * the "system identifier" should be null (different from empty)
    /// * the "force quirks" flag should be `false`
    fn init_doctype(&mut self, reader: &R);

    /// Set the _current attribute_ to a new one, starting with empty name and value strings.
    ///
    /// The old attribute, if any, should be put on the _current token_. If an attribute with that
    /// name already exists, WHATWG says the new one should be ignored and a
    /// [`crate::Error::DuplicateAttribute`] error should be emitted.
    ///
    /// If the current token is an end tag token, a [`crate::Error::EndTagWithAttributes`] error should be
    /// emitted.
    ///
    /// If the current token is no tag at all, this method may panic.
    fn init_attribute(&mut self, reader: &R);

    /// Append a string to the current attribute's name.
    ///
    /// If there is no current attribute, this method may panic.
    fn push_attribute_name(&mut self, s: &[u8], reader: &R);

    /// Append a string to the current attribute's value.
    ///
    /// If there is no current attribute, this method may panic.
    fn push_attribute_value(&mut self, s: &[u8], reader: &R);

    /// Assuming the _current token_ is a doctype, set its "public identifier" to the given string.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn set_doctype_public_identifier(&mut self, value: &[u8], reader: &R);

    /// Assuming the _current token_ is a doctype, set its "system identifier" to the given string.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn set_doctype_system_identifier(&mut self, value: &[u8], reader: &R);

    /// Assuming the _current token_ is a doctype, append a string to its "public identifier" to the given string.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn push_doctype_public_identifier(&mut self, s: &[u8], reader: &R);

    /// Assuming the _current token_ is a doctype, append a string to its "system identifier" to the given string.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn push_doctype_system_identifier(&mut self, s: &[u8], reader: &R);

    /// Start a new tag/comment or something starting with `<`.
    fn start_open_tag(&mut self, _reader: &R);

    /// Return true if all of these hold. Return false otherwise.
    ///
    /// * the _current token_ is an end tag
    /// * the _last start tag_ exists
    /// * the current end tag token's name equals to the last start tag's name.
    ///
    /// See also [WHATWG's definition of "appropriate end tag
    /// token"](https://html.spec.whatwg.org/#appropriate-end-tag-token).
    fn current_is_appropriate_end_tag_token(&mut self) -> bool;

    /// By default, this always returns false and thus
    /// all CDATA sections are tokenized as bogus comments.
    ///
    /// See [markup declaration open
    /// state](https://html.spec.whatwg.org/multipage/#markup-declaration-open-state).
    fn adjusted_current_node_present_but_not_in_html_namespace(&mut self) -> bool {
        false
    }
}

/// Take an educated guess at the next state using the name of a just-now emitted start tag.
///
/// This can be used to implement [`Emitter::emit_current_tag`] for most HTML scraping applications,
/// but is unsuitable for implementing a browser.
///
/// The mapping was inspired by `lol-html` which has additional safeguards to detect ambiguous
/// parsing state: <https://github.com/cloudflare/lol-html/blob/f40a9f767c41caf07851548d7470649a6019548c/src/parser/tree_builder_simulator/mod.rs#L73-L86>
#[must_use]
pub fn naive_next_state(tag_name: &[u8]) -> Option<State> {
    match tag_name {
        b"textarea" | b"title" => Some(State::RcData),
        b"plaintext" => Some(State::PlainText),
        b"script" => Some(State::ScriptData),
        b"style" | b"iframe" | b"xmp" | b"noembed" | b"noframe" | b"noscript" => {
            Some(State::RawText)
        }
        _ => None,
    }
}
