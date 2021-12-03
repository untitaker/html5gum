//! Source code spans.
use std::{
    collections::{btree_map::Entry, BTreeSet, VecDeque},
    marker::PhantomData,
    mem,
};

use crate::{Attribute, Doctype, Emitter, EndTag, Error, Reader, StartTag, Token};

type Span = std::ops::Range<usize>;

/// A trait to be implemented by readers that track their own position.
pub trait GetPos {
    /// Returns the byte index of the current position.
    fn get_pos(&self) -> usize;
}

/// Wraps a [`Reader`] so that it implements [`GetPos`].
pub struct PosTracker<R> {
    /// The wrapped reader.
    pub reader: R,
    /// The current position.
    pub position: usize,
}

impl<R> GetPos for PosTracker<R> {
    fn get_pos(&self) -> usize {
        self.position
    }
}

impl<R: Reader> Reader for PosTracker<R> {
    type Error = R::Error;

    fn read_char(&mut self) -> Result<Option<char>, Self::Error> {
        match self.reader.read_char()? {
            Some(char) => {
                self.position += char.len_utf8();
                Ok(Some(char))
            }
            None => Ok(None),
        }
    }

    fn try_read_string(&mut self, s: &str, case_sensitive: bool) -> Result<bool, Self::Error> {
        match self.reader.try_read_string(s, case_sensitive)? {
            true => {
                self.position += s.len();
                Ok(true)
            }
            false => Ok(false),
        }
    }
}

/// The default implementation of [`crate::Emitter`], used to produce ("emit") tokens.
pub struct SpanEmitter<R> {
    current_characters: String,
    current_token: Option<Token<Span>>,
    last_start_tag: String,
    current_attribute: Option<(String, Attribute<Span>)>,
    seen_attributes: BTreeSet<String>,
    emitted_tokens: VecDeque<Token<Span>>,
    reader: PhantomData<R>,
    attr_in_end_tag_span: Span,
}

impl<R> Default for SpanEmitter<R> {
    fn default() -> Self {
        SpanEmitter {
            current_characters: String::new(),
            current_token: None,
            last_start_tag: String::new(),
            current_attribute: None,
            seen_attributes: BTreeSet::new(),
            emitted_tokens: VecDeque::new(),
            reader: PhantomData::default(),
            attr_in_end_tag_span: Span::default(),
        }
    }
}

impl<R: GetPos> SpanEmitter<R> {
    fn emit_token(&mut self, token: Token<Span>) {
        self.flush_current_characters();
        self.emitted_tokens.push_front(token);
    }

    fn flush_current_attribute(&mut self) {
        if let Some((k, v)) = self.current_attribute.take() {
            match self.current_token {
                Some(Token::StartTag(ref mut tag)) => match tag.attributes.entry(k) {
                    Entry::Vacant(vacant) => {
                        vacant.insert(v);
                    }
                    Entry::Occupied(occupied) => {
                        let span = occupied.get().name_span.clone();
                        self.emit_error_span(Error::DuplicateAttribute, span);
                    }
                },
                Some(Token::EndTag(_)) => {
                    self.attr_in_end_tag_span = v.name_span.clone();
                    if !self.seen_attributes.insert(k) {
                        self.emit_error_span(Error::DuplicateAttribute, v.name_span);
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

    fn emit_error_span(&mut self, error: Error, span: Span) {
        // bypass character flushing in self.emit_token: we don't need the error location to be
        // that exact
        self.emitted_tokens.push_front(Token::Error { error, span });
    }
}

impl<R: GetPos> Emitter<R> for SpanEmitter<R> {
    type Token = Token<Span>;

    fn set_last_start_tag(&mut self, last_start_tag: Option<&str>) {
        self.last_start_tag.clear();
        self.last_start_tag
            .push_str(last_start_tag.unwrap_or_default());
    }

    fn emit_eof(&mut self) {
        self.flush_current_characters();
    }

    fn emit_error(&mut self, error: Error, reader: &R) {
        self.emit_error_span(error, reader.get_pos() - 1..reader.get_pos() - 1)
    }

    fn pop_token(&mut self) -> Option<Self::Token> {
        self.emitted_tokens.pop_back()
    }

    fn emit_string(&mut self, s: &str) {
        self.current_characters.push_str(s);
    }

    fn init_start_tag(&mut self, reader: &R) {
        self.current_token = Some(Token::StartTag(StartTag {
            name_span: reader.get_pos() - 1..reader.get_pos() - 1,
            ..Default::default()
        }));
    }
    fn init_end_tag(&mut self, reader: &R) {
        self.current_token = Some(Token::EndTag(EndTag {
            name_span: reader.get_pos() - 1..reader.get_pos() - 1,
            ..Default::default()
        }));
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
                    self.emit_error_span(
                        Error::EndTagWithAttributes,
                        self.attr_in_end_tag_span.clone(),
                    );
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

    fn set_self_closing(&mut self, reader: &R) {
        let tag = self.current_token.as_mut().unwrap();
        match tag {
            Token::StartTag(StartTag {
                ref mut self_closing,
                ..
            }) => {
                *self_closing = true;
            }
            Token::EndTag(_) => {
                self.emit_error(Error::EndTagWithTrailingSolidus, reader);
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
            Some(Token::StartTag(StartTag {
                ref mut name,
                ref mut name_span,
                ..
            })) => {
                name.push_str(s);
                name_span.end += s.len();
            }
            Some(Token::EndTag(EndTag {
                ref mut name,
                ref mut name_span,
                ..
            })) => {
                name.push_str(s);
                name_span.end += s.len();
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

    fn init_attribute_name(&mut self, reader: &R) {
        self.flush_current_attribute();
        self.current_attribute = Some((
            String::new(),
            Attribute {
                name_span: reader.get_pos() - 1..reader.get_pos() - 1,
                ..Default::default()
            },
        ));
    }

    fn init_attribute_value(&mut self, reader: &R, quoted: bool) {
        let current_attr = self.current_attribute.as_mut().unwrap();
        let offset = if quoted { 0 } else { 1 };
        current_attr.1.value_span = reader.get_pos() - offset..reader.get_pos() - offset;
    }

    fn push_attribute_name(&mut self, s: &str) {
        let current_attr = self.current_attribute.as_mut().unwrap();
        current_attr.0.push_str(s);
        current_attr.1.name_span.end += s.len();
    }
    fn push_attribute_value(&mut self, s: &str) {
        let current_attr = self.current_attribute.as_mut().unwrap();
        current_attr.1.value.push_str(s);
        current_attr.1.value_span.end += s.len();
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
