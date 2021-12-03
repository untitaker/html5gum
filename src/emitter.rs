use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::mem;

use crate::Error;

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
    fn set_last_start_tag(&mut self, last_start_tag: Option<&str>);

    /// The state machine has reached the end of the file. It will soon call `pop_token` for the
    /// last time.
    fn emit_eof(&mut self);

    /// A (probably recoverable) parsing error has occured.
    fn emit_error(&mut self, error: Error, reader: &R);

    /// After every state change, the tokenizer calls this method to retrieve a new token that can
    /// be returned via the tokenizer's iterator interface.
    fn pop_token(&mut self) -> Option<Self::Token>;

    /// Emit a bunch of plain characters as character tokens.
    fn emit_string(&mut self, c: &str);

    /// Set the _current token_ to a start tag.
    fn init_start_tag(&mut self, reader: &R);

    /// Set the _current token_ to an end tag.
    fn init_end_tag(&mut self, reader: &R);

    /// Set the _current token_ to a comment.
    fn init_comment(&mut self, reader: &R);

    /// Emit the _current token_, assuming it is a tag.
    ///
    /// Also get the current attribute and append it to the to-be-emitted tag. See docstring for
    /// [`Emitter::init_attribute_name`] for how duplicates should be handled.
    ///
    /// If a start tag is emitted, update the _last start tag_.
    ///
    /// If the current token is not a start/end tag, this method may panic.
    fn emit_current_tag(&mut self);

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
    fn set_self_closing(&mut self, reader: &R);

    /// Assuming the _current token_ is a doctype, set its "force quirks" flag to true.
    ///
    /// If the current token is not a doctype, this method pay panic.
    fn set_force_quirks(&mut self);

    /// Assuming the _current token_ is a start/end tag, append a string to the current tag's name.
    ///
    /// If the current token is not a start or end tag, this method may panic.
    fn push_tag_name(&mut self, s: &str);

    /// Assuming the _current token_ is a comment, append a string to the comment's contents.
    ///
    /// If the current token is not a comment, this method may panic.
    fn push_comment(&mut self, s: &str);

    /// Assuming the _current token_ is a doctype, append a string to the doctype's name.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn push_doctype_name(&mut self, s: &str);

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
    fn init_attribute_name(&mut self, reader: &R);

    /// Called before the first push_attribute_value call.
    /// If the value is wrappend in double or single quotes `quoted` is set to true, otherwise false.
    ///
    /// If there is no current attribute, this method may panic.
    #[allow(unused_variables)]
    fn init_attribute_value(&mut self, reader: &R, quoted: bool) {}

    /// Append a string to the current attribute's name.
    ///
    /// If there is no current attribute, this method may panic.
    fn push_attribute_name(&mut self, s: &str);

    /// Append a string to the current attribute's value.
    ///
    /// If there is no current attribute, this method may panic.
    fn push_attribute_value(&mut self, s: &str);

    /// Assuming the _current token_ is a doctype, set its "public identifier" to the given string.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn set_doctype_public_identifier(&mut self, value: &str);

    /// Assuming the _current token_ is a doctype, set its "system identifier" to the given string.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn set_doctype_system_identifier(&mut self, value: &str);

    /// Assuming the _current token_ is a doctype, append a string to its "public identifier" to the given string.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn push_doctype_public_identifier(&mut self, s: &str);

    /// Assuming the _current token_ is a doctype, append a string to its "system identifier" to the given string.
    ///
    /// If the current token is not a doctype, this method may panic.
    fn push_doctype_system_identifier(&mut self, s: &str);

    /// Return true if all of these hold. Return false otherwise.
    ///
    /// * the _current token_ is an end tag
    /// * the _last start tag_ exists
    /// * the current end tag token's name equals to the last start tag's name.
    ///
    /// See also [WHATWG's definition of "appropriate end tag
    /// token"](https://html.spec.whatwg.org/#appropriate-end-tag-token).
    fn current_is_appropriate_end_tag_token(&mut self) -> bool;
}

/// The default implementation of [`crate::Emitter`], used to produce ("emit") tokens.
pub struct DefaultEmitter<R, S> {
    current_characters: String,
    current_token: Option<Token<S>>,
    last_start_tag: String,
    current_attribute: Option<(String, Attribute<S>)>,
    seen_attributes: BTreeSet<String>,
    emitted_tokens: VecDeque<Token<S>>,
    reader: PhantomData<R>,
}

impl<R, S> Default for DefaultEmitter<R, S> {
    fn default() -> Self {
        DefaultEmitter {
            current_characters: String::new(),
            current_token: None,
            last_start_tag: String::new(),
            current_attribute: None,
            seen_attributes: BTreeSet::new(),
            emitted_tokens: VecDeque::new(),
            reader: PhantomData::default(),
        }
    }
}

impl<R> DefaultEmitter<R, ()> {
    fn emit_token(&mut self, token: Token<()>) {
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

    fn emit_error(&mut self, error: Error) {
        // bypass character flushing in self.emit_token: we don't need the error location to be
        // that exact
        self.emitted_tokens
            .push_front(Token::Error { error, span: () });
    }
}

impl<R> Emitter<R> for DefaultEmitter<R, ()> {
    type Token = Token<()>;

    fn set_last_start_tag(&mut self, last_start_tag: Option<&str>) {
        self.last_start_tag.clear();
        self.last_start_tag
            .push_str(last_start_tag.unwrap_or_default());
    }

    fn emit_eof(&mut self) {
        self.flush_current_characters();
    }

    fn emit_error(&mut self, error: Error, _reader: &R) {
        self.emit_error(error);
    }

    fn pop_token(&mut self) -> Option<Self::Token> {
        self.emitted_tokens.pop_back()
    }

    fn emit_string(&mut self, s: &str) {
        self.current_characters.push_str(s);
    }

    fn init_start_tag(&mut self, _reader: &R) {
        self.current_token = Some(Token::StartTag(Default::default()));
    }
    fn init_end_tag(&mut self, _reader: &R) {
        self.current_token = Some(Token::EndTag(Default::default()));
        self.seen_attributes.clear();
    }

    fn init_comment(&mut self, _reader: &R) {
        self.current_token = Some(Token::Comment(String::new()));
    }
    fn emit_current_tag(&mut self) {
        self.flush_current_attribute();
        let mut token = self.current_token.take().unwrap();
        match token {
            Token::EndTag(_) => {
                if !self.seen_attributes.is_empty() {
                    self.emit_error(Error::EndTagWithAttributes);
                }
                self.seen_attributes.clear();
            }
            Token::StartTag(ref mut _tag) => {
                self.set_last_start_tag(Some(&_tag.name));
            }
            _ => debug_assert!(false),
        }
        self.emit_token(token);
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

    fn set_self_closing(&mut self, _reader: &R) {
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
    fn push_tag_name(&mut self, s: &str) {
        match self.current_token {
            Some(Token::StartTag(StartTag { ref mut name, .. })) => {
                name.push_str(s);
            }
            Some(Token::EndTag(EndTag { ref mut name, .. })) => {
                name.push_str(s);
            }
            _ => debug_assert!(false),
        }
    }

    fn push_comment(&mut self, s: &str) {
        match self.current_token {
            Some(Token::Comment(ref mut data)) => data.push_str(s),
            _ => debug_assert!(false),
        }
    }

    fn push_doctype_name(&mut self, s: &str) {
        match self.current_token {
            Some(Token::Doctype(ref mut doctype)) => doctype.name.push_str(s),
            _ => debug_assert!(false),
        }
    }
    fn init_doctype(&mut self, _reader: &R) {
        self.current_token = Some(Token::Doctype(Doctype {
            name: String::new(),
            force_quirks: false,
            public_identifier: None,
            system_identifier: None,
        }));
    }

    fn init_attribute_name(&mut self, _reader: &R) {
        self.flush_current_attribute();
        self.current_attribute = Some((String::new(), Attribute::default()));
    }
    fn push_attribute_name(&mut self, s: &str) {
        self.current_attribute.as_mut().unwrap().0.push_str(s);
    }
    fn push_attribute_value(&mut self, s: &str) {
        self.current_attribute.as_mut().unwrap().1.value.push_str(s);
    }
    fn set_doctype_public_identifier(&mut self, value: &str) {
        if let Some(Token::Doctype(Doctype {
            ref mut public_identifier,
            ..
        })) = self.current_token
        {
            *public_identifier = Some(value.to_owned());
        } else {
            debug_assert!(false);
        }
    }
    fn set_doctype_system_identifier(&mut self, value: &str) {
        if let Some(Token::Doctype(Doctype {
            ref mut system_identifier,
            ..
        })) = self.current_token
        {
            *system_identifier = Some(value.to_owned());
        } else {
            debug_assert!(false);
        }
    }
    fn push_doctype_public_identifier(&mut self, s: &str) {
        if let Some(Token::Doctype(Doctype {
            public_identifier: Some(ref mut id),
            ..
        })) = self.current_token
        {
            id.push_str(s);
        } else {
            debug_assert!(false);
        }
    }
    fn push_doctype_system_identifier(&mut self, s: &str) {
        if let Some(Token::Doctype(Doctype {
            system_identifier: Some(ref mut id),
            ..
        })) = self.current_token
        {
            id.push_str(s);
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
#[derive(Debug, Default, Eq, PartialEq)]
pub struct StartTag<S> {
    /// Whether this tag is self-closing. If it is self-closing, no following [`EndTag`] should be
    /// expected.
    pub self_closing: bool,

    /// The start tag's name, such as `"p"` or `"a"`.
    pub name: String,

    /// A mapping for any HTML attributes this start tag may have.
    ///
    /// Duplicate attributes are ignored after the first one as per WHATWG spec. Implement your own
    /// [`Emitter`] to tweak this behavior.
    pub attributes: BTreeMap<String, Attribute<S>>,

    /// The source code span of the tag name.
    pub name_span: S,
}

/// A HTML attribute value (plus spans).
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Attribute<S> {
    /// The value of the attribute.
    pub value: String,

    /// The source code span of the attribute name.
    pub name_span: S,

    /// The source code span of the attribute value.
    pub value_span: S,
}

/// A HTML end/close tag, such as `</p>` or `</a>`.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct EndTag<S> {
    /// The ending tag's name, such as `"p"` or `"a"`.
    pub name: String,

    /// The source code span of the tag name.
    pub name_span: S,
}

/// A doctype. Some examples:
///
/// * `<!DOCTYPE {name}>`
/// * `<!DOCTYPE {name} PUBLIC '{public_identifier}'>`
/// * `<!DOCTYPE {name} SYSTEM '{system_identifier}'>`
/// * `<!DOCTYPE {name} PUBLIC '{public_identifier}' '{system_identifier}'>`
#[derive(Debug, Eq, PartialEq)]
pub struct Doctype {
    /// The ["force quirks"](https://html.spec.whatwg.org/#force-quirks-flag) flag.
    pub force_quirks: bool,

    /// The doctype's name. For HTML documents this is "html".
    pub name: String,

    /// The doctype's public identifier.
    pub public_identifier: Option<String>,

    /// The doctype's system identifier.
    pub system_identifier: Option<String>,
}

/// The token type used by default. You can define your own token type by implementing the
/// [`crate::Emitter`] trait and using [`crate::Tokenizer::new_with_emitter`].
#[derive(Debug, Eq, PartialEq)]
pub enum Token<S> {
    /// A HTML start tag.
    StartTag(StartTag<S>),
    /// A HTML end tag.
    EndTag(EndTag<S>),
    /// A literal string.
    String(String),
    /// A HTML comment.
    Comment(String),
    /// A HTML doctype declaration.
    Doctype(Doctype),
    /// A HTML parsing error.
    ///
    /// Can be skipped over, the tokenizer is supposed to recover from the error and continues with
    /// more tokens afterward.
    Error {
        /// What kind of error occured.
        error: Error,
        /// The source code span of the error.
        span: S,
    },
}
