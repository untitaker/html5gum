//! This is a demonstration of how one would wire up the html5ever tree builder to html5gum's
//! tokenizer. The result is a slow mess, but it works!
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    Doctype, Tag, TagKind, Token as Html5everToken, TokenSink, TokenSinkResult,
};
use html5ever::tree_builder::{TreeBuilder, TreeBuilderOpts};
use html5ever::{Attribute, QualName};
use markup5ever_rcdom::RcDom;

use html5gum::{DefaultEmitter, Emitter, Error, IoReader, State, Token, Tokenizer};

const BOGUS_LINENO: u64 = 1;

struct Html5everEmitter<S: TokenSink> {
    sink: S,
    // TODO: get rid of default emitter, construct html5ever tokens directly
    emitter_inner: DefaultEmitter,
}

impl<S: TokenSink> Emitter for &mut Html5everEmitter<S> {
    type Token = ();

    fn set_last_start_tag(&mut self, last_start_tag: Option<&[u8]>) {
        self.emitter_inner.set_last_start_tag(last_start_tag)
    }

    fn emit_eof(&mut self) {
        self.emitter_inner.emit_eof();
        let _ignored = self
            .sink
            .process_token(Html5everToken::EOFToken, BOGUS_LINENO);
        self.sink.end();
    }

    fn emit_error(&mut self, error: Error) {
        self.emitter_inner.emit_error(error);
    }

    fn pop_token(&mut self) -> Option<()> {
        None
    }

    fn emit_string(&mut self, c: &[u8]) {
        self.emitter_inner.emit_string(c);
    }

    fn init_start_tag(&mut self) {
        self.emitter_inner.init_start_tag();
    }

    fn init_end_tag(&mut self) {
        self.emitter_inner.init_end_tag();
    }

    fn init_comment(&mut self) {
        self.emitter_inner.init_comment();
    }

    fn emit_current_tag(&mut self) -> Option<State> {
        let _ignored = self.emitter_inner.emit_current_tag();
        let token = self.emitter_inner.pop_token()?;
        match self
            .sink
            .process_token(token_to_html5ever(token), BOGUS_LINENO)
        {
            TokenSinkResult::Continue => None,
            TokenSinkResult::Script(_) => {
                unimplemented!("not sure what to do with this")
            }
            TokenSinkResult::Plaintext => Some(State::PlainText),
            TokenSinkResult::RawData(RawKind::Rcdata) => Some(State::RcData),
            TokenSinkResult::RawData(RawKind::Rawtext) => Some(State::RawText),
            TokenSinkResult::RawData(RawKind::ScriptData) => Some(State::ScriptData),
            TokenSinkResult::RawData(RawKind::ScriptDataEscaped(_)) => {
                unimplemented!("not sure what to do with this")
            }
        }
    }

    fn emit_current_comment(&mut self) {
        self.emitter_inner.emit_current_comment();
    }

    fn emit_current_doctype(&mut self) {
        self.emitter_inner.emit_current_doctype();
    }

    fn set_self_closing(&mut self) {
        self.emitter_inner.set_self_closing();
    }

    fn set_force_quirks(&mut self) {
        self.emitter_inner.set_force_quirks();
    }

    fn push_tag_name(&mut self, s: &[u8]) {
        self.emitter_inner.push_tag_name(s);
    }

    fn push_comment(&mut self, s: &[u8]) {
        self.emitter_inner.push_comment(s);
    }

    fn push_doctype_name(&mut self, s: &[u8]) {
        self.emitter_inner.push_doctype_name(s);
    }

    fn init_doctype(&mut self) {
        self.emitter_inner.init_doctype();
    }

    fn init_attribute(&mut self) {
        self.emitter_inner.init_attribute();
    }

    fn push_attribute_name(&mut self, s: &[u8]) {
        self.emitter_inner.push_attribute_name(s);
    }

    fn push_attribute_value(&mut self, s: &[u8]) {
        self.emitter_inner.push_attribute_value(s);
    }

    fn set_doctype_public_identifier(&mut self, value: &[u8]) {
        self.emitter_inner.set_doctype_public_identifier(value);
    }

    fn set_doctype_system_identifier(&mut self, value: &[u8]) {
        self.emitter_inner.set_doctype_system_identifier(value);
    }

    fn push_doctype_public_identifier(&mut self, value: &[u8]) {
        self.emitter_inner.push_doctype_public_identifier(value);
    }

    fn push_doctype_system_identifier(&mut self, value: &[u8]) {
        self.emitter_inner.push_doctype_system_identifier(value);
    }

    fn current_is_appropriate_end_tag_token(&mut self) -> bool {
        self.emitter_inner.current_is_appropriate_end_tag_token()
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&mut self) -> bool {
        self.sink
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

fn token_to_html5ever(token: Token) -> Html5everToken {
    match token {
        Token::StartTag(tag) => Html5everToken::TagToken(Tag {
            kind: TagKind::StartTag,
            name: String::from_utf8_lossy(&*tag.name).into_owned().into(),
            self_closing: tag.self_closing,
            attrs: tag
                .attributes
                .into_iter()
                .map(|(key, value)| Attribute {
                    name: QualName::new(
                        None,
                        Default::default(),
                        String::from_utf8_lossy(&*key).into_owned().into(),
                    ),
                    value: String::from_utf8_lossy(&*value).into_owned().into(),
                })
                .collect(),
        }),
        Token::EndTag(tag) => Html5everToken::TagToken(Tag {
            kind: TagKind::EndTag,
            name: String::from_utf8_lossy(&*tag.name).into_owned().into(),
            self_closing: false,
            attrs: Vec::new(),
        }),
        Token::String(s) => {
            Html5everToken::CharacterTokens(String::from_utf8_lossy(&*s).into_owned().into())
        }
        Token::Comment(c) => {
            Html5everToken::CommentToken(String::from_utf8_lossy(&*c).into_owned().into())
        }
        Token::Doctype(doctype) => Html5everToken::DoctypeToken(Doctype {
            name: Some(&*doctype.name)
                .filter(|x| !x.is_empty())
                .map(|x| String::from_utf8_lossy(x).into_owned().into()),
            public_id: doctype
                .public_identifier
                .map(|x| String::from_utf8_lossy(&*x).into_owned().into()),
            system_id: doctype
                .system_identifier
                .map(|x| String::from_utf8_lossy(&*x).into_owned().into()),
            force_quirks: doctype.force_quirks,
        }),
        Token::Error(err) => Html5everToken::ParseError(err.as_str().into()),
    }
}

fn main() {
    let mut emitter = Html5everEmitter {
        sink: TreeBuilder::new(RcDom::default(), TreeBuilderOpts::default()),
        emitter_inner: DefaultEmitter::default(),
    };

    let reader = IoReader::new(std::io::stdin().lock());
    for _ in Tokenizer::new_with_emitter(reader, &mut emitter).flatten() {}

    println!("{:#?}", emitter.sink.sink.document);
}
