use std::borrow::{Borrow, BorrowMut};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::fmt::{Debug, Formatter};
use std::mem;
use std::ops::{Deref, DerefMut};

use crate::{Error, State};

/// A wrapper around a bytestring.
///
/// This newtype only exists to provide a nicer `Debug` impl
#[derive(Clone, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct HtmlString(pub Vec<u8>);

impl Deref for HtmlString {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for HtmlString {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Debug for HtmlString {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "b\"")?;
        for &byte in &self.0 {
            for ch in std::ascii::escape_default(byte) {
                write!(f, "{}", ch as char)?;
            }
        }

        write!(f, "\"")
    }
}

impl Borrow<[u8]> for HtmlString {
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

impl BorrowMut<[u8]> for HtmlString {
    fn borrow_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

#[test]
fn test_borrowing() {
    // demonstrate a usecase for Borrow/BorrowMut
    let tag = StartTag::default();
    assert!(tag.attributes.get(b"href".as_slice()).is_none());
}

impl From<Vec<u8>> for HtmlString {
    fn from(vec: Vec<u8>) -> HtmlString {
        HtmlString(vec)
    }
}

impl From<HtmlString> for Vec<u8> {
    fn from(other: HtmlString) -> Vec<u8> {
        other.0
    }
}

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
pub trait Emitter {
    /// The token type emitted by this emitter. This controls what type of values the [`crate::Tokenizer`]
    /// yields when used as an iterator.
    type Token;

    /// Set the name of the _last start tag_.
    ///
    /// This is primarily for testing purposes. This is *not* supposed to override the tag name of
    /// the current tag.
    fn set_last_start_tag(&mut self, last_start_tag: Option<&[u8]>);

    /// The state machine has reached the end of the file. It will soon call `pop_token` for the
    /// last time.
    fn emit_eof(&mut self);

    /// A (probably recoverable) parsing error has occured.
    fn emit_error(&mut self, error: Error);

    /// After every state change, the tokenizer calls this method to retrieve a new token that can
    /// be returned via the tokenizer's iterator interface.
    fn pop_token(&mut self) -> Option<Self::Token>;

    /// Emit a bunch of plain characters as character tokens.
    fn emit_string(&mut self, c: &[u8]);

    /// Set the _current token_ to a start tag.
    fn init_start_tag(&mut self);

    /// Set the _current token_ to an end tag.
    fn init_end_tag(&mut self);

    /// Set the _current token_ to a comment.
    fn init_comment(&mut self);

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
    fn emit_current_tag(&mut self) -> Option<State>;

    /// Emit the _current token_, assuming it is a comment.
    ///
    /// If the current token is not a comment, this method may panic.
    fn emit_current_comment(&mut self);

    /// Emit the _current token_, assuming it is a doctype.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn emit_current_doctype(&mut self);

    /// Assuming the _current token_ is a start tag, set the self-closing flag.
    ///
    /// If the current token is not a start or end tag, this method may panic.
    ///
    /// If the current token is an end tag, the emitter should emit the
    /// [`crate::Error::EndTagWithTrailingSolidus`] error.
    fn set_self_closing(&mut self);

    /// Assuming the _current token_ is a doctype, set its "force quirks" flag to true.
    ///
    /// If the current token is not a doctype, this method pay panic.
    fn set_force_quirks(&mut self);

    /// Assuming the _current token_ is a start/end tag, append a string to the current tag's name.
    ///
    /// If the current token is not a start or end tag, this method may panic.
    fn push_tag_name(&mut self, s: &[u8]);

    /// Assuming the _current token_ is a comment, append a string to the comment's contents.
    ///
    /// If the current token is not a comment, this method may panic.
    fn push_comment(&mut self, s: &[u8]);

    /// Assuming the _current token_ is a doctype, append a string to the doctype's name.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn push_doctype_name(&mut self, s: &[u8]);

    /// Set the _current token_ to a new doctype token:
    ///
    /// * the name should be empty
    /// * the "public identifier" should be null (different from empty)
    /// * the "system identifier" should be null (different from empty)
    /// * the "force quirks" flag should be `false`
    fn init_doctype(&mut self);

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
    fn init_attribute(&mut self);

    /// Append a string to the current attribute's name.
    ///
    /// If there is no current attribute, this method may panic.
    fn push_attribute_name(&mut self, s: &[u8]);

    /// Append a string to the current attribute's value.
    ///
    /// If there is no current attribute, this method may panic.
    fn push_attribute_value(&mut self, s: &[u8]);

    /// Assuming the _current token_ is a doctype, set its "public identifier" to the given string.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn set_doctype_public_identifier(&mut self, value: &[u8]);

    /// Assuming the _current token_ is a doctype, set its "system identifier" to the given string.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn set_doctype_system_identifier(&mut self, value: &[u8]);

    /// Assuming the _current token_ is a doctype, append a string to its "public identifier" to the given string.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn push_doctype_public_identifier(&mut self, s: &[u8]);

    /// Assuming the _current token_ is a doctype, append a string to its "system identifier" to the given string.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn push_doctype_system_identifier(&mut self, s: &[u8]);

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
/// parsing state.
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

/// The default implementation of [`crate::Emitter`], used to produce ("emit") tokens.
#[derive(Debug, Default)]
pub struct DefaultEmitter {
    current_characters: HtmlString,
    current_token: Option<Token>,
    last_start_tag: HtmlString,
    current_attribute: Option<(HtmlString, HtmlString)>,
    seen_attributes: BTreeSet<HtmlString>,
    emitted_tokens: VecDeque<Token>,
    switch_states: bool,
}

impl DefaultEmitter {
    /// Whether to use [`naive_next_state`] to switch states automatically.
    ///
    /// The default is off.
    pub fn switch_states(&mut self, yes: bool) {
        self.switch_states = yes;
    }

    fn emit_token(&mut self, token: Token) {
        self.flush_current_characters();
        self.emitted_tokens.push_front(token);
    }

    fn flush_current_attribute(&mut self) {
        if let Some((k, v)) = self.current_attribute.take() {
            match self.current_token {
                Some(Token::StartTag(ref mut tag)) => {
                    let mut error = None;
                    tag.attributes
                        .entry(k)
                        .and_modify(|_| {
                            error = Some(Error::DuplicateAttribute);
                        })
                        .or_insert(v);

                    if let Some(e) = error {
                        self.emit_error(e);
                    }
                }
                Some(Token::EndTag(_)) => {
                    if !self.seen_attributes.insert(k) {
                        self.emit_error(Error::DuplicateAttribute);
                    }
                }
                _ => {
                    debug_assert!(false);
                }
            }
        }
    }

    fn flush_current_characters(&mut self) {
        if self.current_characters.is_empty() {
            return;
        }

        let s = mem::take(&mut self.current_characters);
        self.emit_token(Token::String(s));
    }
}

impl Emitter for DefaultEmitter {
    type Token = Token;

    fn set_last_start_tag(&mut self, last_start_tag: Option<&[u8]>) {
        self.last_start_tag.clear();
        self.last_start_tag
            .extend(last_start_tag.unwrap_or_default());
    }

    fn emit_eof(&mut self) {
        self.flush_current_characters();
    }

    fn emit_error(&mut self, error: Error) {
        // bypass character flushing in self.emit_token: we don't need the error location to be
        // that exact
        self.emitted_tokens.push_front(Token::Error(error));
    }

    fn pop_token(&mut self) -> Option<Self::Token> {
        self.emitted_tokens.pop_back()
    }

    fn emit_string(&mut self, s: &[u8]) {
        self.current_characters.extend(s);
    }

    fn init_start_tag(&mut self) {
        self.current_token = Some(Token::StartTag(StartTag::default()));
    }
    fn init_end_tag(&mut self) {
        self.current_token = Some(Token::EndTag(EndTag::default()));
        self.seen_attributes.clear();
    }

    fn init_comment(&mut self) {
        self.current_token = Some(Token::Comment(HtmlString::default()));
    }
    fn emit_current_tag(&mut self) -> Option<State> {
        self.flush_current_attribute();
        let mut token = self.current_token.take().unwrap();
        match token {
            Token::EndTag(_) => {
                if !self.seen_attributes.is_empty() {
                    self.emit_error(Error::EndTagWithAttributes);
                }
                self.seen_attributes.clear();
                self.set_last_start_tag(None);
            }
            Token::StartTag(ref mut tag) => {
                self.set_last_start_tag(Some(&tag.name));
            }
            _ => debug_assert!(false),
        }
        self.emit_token(token);
        if self.switch_states {
            naive_next_state(&*self.last_start_tag)
        } else {
            None
        }
    }
    fn emit_current_comment(&mut self) {
        let comment = self.current_token.take().unwrap();
        debug_assert!(matches!(comment, Token::Comment(_)));
        self.emit_token(comment);
    }

    fn emit_current_doctype(&mut self) {
        let doctype = self.current_token.take().unwrap();
        debug_assert!(matches!(doctype, Token::Doctype(_)));
        self.emit_token(doctype);
    }

    fn set_self_closing(&mut self) {
        let tag = self.current_token.as_mut().unwrap();
        match tag {
            Token::StartTag(StartTag {
                ref mut self_closing,
                ..
            }) => {
                *self_closing = true;
            }
            Token::EndTag(_) => {
                self.emit_error(Error::EndTagWithTrailingSolidus);
            }
            _ => {
                debug_assert!(false);
            }
        }
    }
    fn set_force_quirks(&mut self) {
        match self.current_token {
            Some(Token::Doctype(ref mut doctype)) => doctype.force_quirks = true,
            _ => debug_assert!(false),
        }
    }
    fn push_tag_name(&mut self, s: &[u8]) {
        match self.current_token {
            Some(
                Token::StartTag(StartTag { ref mut name, .. })
                | Token::EndTag(EndTag { ref mut name, .. }),
            ) => {
                name.extend(s);
            }
            _ => debug_assert!(false),
        }
    }

    fn push_comment(&mut self, s: &[u8]) {
        match self.current_token {
            Some(Token::Comment(ref mut data)) => data.extend(s),
            _ => debug_assert!(false),
        }
    }

    fn push_doctype_name(&mut self, s: &[u8]) {
        match self.current_token {
            Some(Token::Doctype(ref mut doctype)) => doctype.name.extend(s),
            _ => debug_assert!(false),
        }
    }
    fn init_doctype(&mut self) {
        self.current_token = Some(Token::Doctype(Doctype {
            name: HtmlString::default(),
            force_quirks: false,
            public_identifier: None,
            system_identifier: None,
        }));
    }

    fn init_attribute(&mut self) {
        self.flush_current_attribute();
        self.current_attribute = Some(Default::default());
    }
    fn push_attribute_name(&mut self, s: &[u8]) {
        self.current_attribute.as_mut().unwrap().0.extend(s);
    }
    fn push_attribute_value(&mut self, s: &[u8]) {
        self.current_attribute.as_mut().unwrap().1.extend(s);
    }
    fn set_doctype_public_identifier(&mut self, value: &[u8]) {
        if let Some(Token::Doctype(Doctype {
            ref mut public_identifier,
            ..
        })) = self.current_token
        {
            *public_identifier = Some(value.to_vec().into());
        } else {
            debug_assert!(false);
        }
    }
    fn set_doctype_system_identifier(&mut self, value: &[u8]) {
        if let Some(Token::Doctype(Doctype {
            ref mut system_identifier,
            ..
        })) = self.current_token
        {
            *system_identifier = Some(value.to_vec().into());
        } else {
            debug_assert!(false);
        }
    }
    fn push_doctype_public_identifier(&mut self, s: &[u8]) {
        if let Some(Token::Doctype(Doctype {
            public_identifier: Some(ref mut id),
            ..
        })) = self.current_token
        {
            id.extend(s);
        } else {
            debug_assert!(false);
        }
    }
    fn push_doctype_system_identifier(&mut self, s: &[u8]) {
        if let Some(Token::Doctype(Doctype {
            system_identifier: Some(ref mut id),
            ..
        })) = self.current_token
        {
            id.extend(s);
        } else {
            debug_assert!(false);
        }
    }

    fn current_is_appropriate_end_tag_token(&mut self) -> bool {
        match self.current_token {
            Some(Token::EndTag(ref tag)) => {
                !self.last_start_tag.is_empty() && self.last_start_tag == tag.name
            }
            _ => false,
        }
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
