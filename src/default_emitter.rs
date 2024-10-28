use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::mem;

use crate::{naive_next_state, Emitter, Error, HtmlString, State};

/// The default implementation of [`crate::Emitter`], used to produce ("emit") tokens.
#[derive(Debug, Default)]
pub struct DefaultEmitter {
    current_characters: HtmlString,
    current_token: Option<Token>,
    last_start_tag: HtmlString,
    current_attribute: Option<(HtmlString, HtmlString)>,
    seen_attributes: BTreeSet<HtmlString>,
    emitted_tokens: VecDeque<Token>,
    naively_switch_states: bool,
}

impl DefaultEmitter {
    /// Whether to use [`naive_next_state`] to switch states automatically.
    ///
    /// The default is off.
    pub fn naively_switch_states(&mut self, yes: bool) {
        self.naively_switch_states = yes;
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
        if self.naively_switch_states {
            naive_next_state(&self.last_start_tag)
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
