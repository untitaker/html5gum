use html5ever::buffer_queue::BufferQueue;
use html5ever::tendril::format_tendril;
use html5ever::tokenizer::{
    TagKind, Token as Token2, TokenSinkResult, TokenizerOpts, TokenizerResult,
};
use html5gum::{Emitter, Reader, Token};

use pretty_assertions::assert_eq;

pub fn run_html5ever(s: &str) {
    let mut reference_tokenizer = html5ever::tokenizer::Tokenizer::new(
        TokenSink {
            testing_tokenizer: html5gum::Tokenizer::new(s),
            carried_over_token: None,
        },
        TokenizerOpts {
            // the html5gum tokenizer does not handle the BOM, and also not discarding a BOM is
            // what the test suite expects, see https://github.com/html5lib/html5lib-tests/issues/2
            discard_bom: false,

            ..Default::default()
        },
    );
    let mut queue = BufferQueue::new();
    queue.push_back(format_tendril!("{}", s));

    assert!(matches!(
        reference_tokenizer.feed(&mut queue),
        TokenizerResult::Done
    ));
    reference_tokenizer.end();
}

struct TokenSink<R: Reader, E: Emitter> {
    testing_tokenizer: html5gum::Tokenizer<R, E>,
    carried_over_token: Option<Token>,
}

impl<R: Reader, E: Emitter<Token = Token>> html5ever::tokenizer::TokenSink for TokenSink<R, E> {
    type Handle = ();

    fn process_token(
        &mut self,
        reference_token: html5ever::tokenizer::Token,
        _line_number: u64,
    ) -> TokenSinkResult<Self::Handle> {
        if matches!(reference_token, Token2::ParseError(_)) {
            // TODO
            return TokenSinkResult::Continue;
        };
        let token = loop {
            let token = self
                .carried_over_token
                .take()
                .or_else(|| self.testing_tokenizer.next().map(|x| x.unwrap()));
            if matches!(token, Some(Token::Error(_))) {
                // TODO
                continue;
            }

            break token;
        };

        match (token, reference_token) {
            (Some(Token::StartTag(tag)), Token2::TagToken(tag2)) => {
                assert_eq!(tag2.kind, TagKind::StartTag);
                assert_eq!(tag.name, tag2.name.as_ref().as_bytes().to_owned());
            }
            (Some(Token::EndTag(tag)), Token2::TagToken(tag2)) => {
                assert_eq!(tag2.kind, TagKind::EndTag);
                assert_eq!(tag.name, tag2.name.as_ref().as_bytes().to_owned());
            }
            (None, Token2::EOFToken) => {}
            (Some(Token::Comment(comment)), Token2::CommentToken(comment2)) => {
                assert_eq!(comment, comment2.as_ref().as_bytes().to_owned());
            }
            (Some(Token::Doctype(doctype)), Token2::DoctypeToken(doctype2)) => {
                assert_eq!(
                    doctype.name,
                    doctype2
                        .name
                        .map(|x| x.as_ref().to_owned().into_bytes())
                        .unwrap_or_default()
                );
                assert_eq!(
                    doctype.public_identifier,
                    doctype2
                        .public_id
                        .map(|x| x.as_ref().to_owned().into_bytes().into())
                );
                assert_eq!(
                    doctype.system_identifier,
                    doctype2
                        .system_id
                        .map(|x| x.as_ref().to_owned().into_bytes().into())
                );
                assert_eq!(doctype.force_quirks, doctype2.force_quirks);
            }
            (Some(Token::String(s)), Token2::NullCharacterToken) => {
                assert_eq!(s[0], b'\0');
                let gum_rest = &s[1..];
                if !gum_rest.is_empty() {
                    assert!(self.carried_over_token.is_none());
                    self.carried_over_token = Some(Token::String(gum_rest.to_owned().into()));
                }
            }
            (Some(Token::String(s)), Token2::CharacterTokens(s2)) => {
                let gum_start = &s[..s2.len()];
                assert_eq!(gum_start, &**s2.as_bytes());
                let gum_rest = &s[s2.len()..];
                if !gum_rest.is_empty() {
                    assert!(self.carried_over_token.is_none());
                    self.carried_over_token = Some(Token::String(gum_rest.to_owned().into()));
                }
            }
            (a, b) => panic!("5gum: {:?}\n5ever: {:?}", a, b),
        }

        TokenSinkResult::Continue
    }
}
