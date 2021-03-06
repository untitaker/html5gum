use html5ever::buffer_queue::BufferQueue;
use html5ever::tendril::format_tendril;
use html5ever::tokenizer::{TagKind, Token as Token2, TokenSinkResult, TokenizerResult};
use html5gum::{Emitter, Reader, Token};

use pretty_assertions::assert_eq;

pub fn run_html5ever(s: &str) {
    let mut reference_tokenizer = html5ever::tokenizer::Tokenizer::new(
        TokenSink {
            testing_tokenizer: html5gum::Tokenizer::new(s),
        },
        Default::default(),
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
}

impl<R: Reader, E: Emitter<Token = Token>> html5ever::tokenizer::TokenSink for TokenSink<R, E> {
    type Handle = ();

    fn process_token(
        &mut self,
        reference_token: html5ever::tokenizer::Token,
        _line_number: u64,
    ) -> TokenSinkResult<Self::Handle> {
        if matches!(
            reference_token,
            Token2::NullCharacterToken | Token2::ParseError(_) | Token2::CharacterTokens(_)
        ) {
            // TODO
            return TokenSinkResult::Continue;
        };
        let token = loop {
            let token = self.testing_tokenizer.next();
            if matches!(token, Some(Ok(Token::Error(_) | Token::String(_)))) {
                // TODO
                continue;
            }

            break token.map(|x| x.unwrap());
        };

        match (token, reference_token) {
            (Some(Token::StartTag(tag)), Token2::TagToken(tag2)) => {
                assert_eq!(tag2.kind, TagKind::StartTag);
                assert_eq!(tag.name, tag2.name.as_ref().as_bytes().to_owned().into());
            }
            (Some(Token::EndTag(tag)), Token2::TagToken(tag2)) => {
                assert_eq!(tag2.kind, TagKind::EndTag);
                assert_eq!(tag.name, tag2.name.as_ref().as_bytes().to_owned().into());
            }
            (None, Token2::EOFToken) => {}
            (Some(Token::Comment(comment)), Token2::CommentToken(comment2)) => {
                assert_eq!(comment, comment2.as_ref().as_bytes().to_owned().into());
            }
            (Some(Token::Doctype(doctype)), Token2::DoctypeToken(doctype2)) => {
                assert_eq!(
                    doctype.name,
                    doctype2
                        .name
                        .map(|x| x.as_ref().to_owned().into_bytes().into())
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
            (a, b) => panic!("5gum: {:?}\n5ever: {:?}", a, b),
        }

        TokenSinkResult::Continue
    }
}
