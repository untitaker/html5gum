use std::env;

use html5ever::buffer_queue::BufferQueue;
use html5ever::tendril::format_tendril;
use html5ever::tokenizer::{TagKind, Token as Token2, TokenSinkResult, TokenizerResult};
use html5gum::{Emitter, Reader, Token};

use pretty_assertions::assert_eq;

pub fn run(s: &str) {
    let mut did_anything = false;

    if env::var("FUZZ_BASIC").unwrap() == "1" {
        let testing_tokenizer = html5gum::Tokenizer::new(s).infallible();
        for _ in testing_tokenizer {}

        did_anything = true;
    }

    if env::var("FUZZ_OLD_HTML5GUM").unwrap() == "1" {
        let reference_tokenizer = html5gum_old::Tokenizer::new(s).infallible();
        let testing_tokenizer = html5gum::Tokenizer::new(s).infallible();
        let testing_tokens: Vec<_> = testing_tokenizer.map(|x| format!("{:?}", x)).collect();
        let reference_tokens: Vec<_> = reference_tokenizer.map(|x| format!("{:?}", x)).collect();
        assert_eq!(testing_tokens, reference_tokens);
        did_anything = true;
    }

    if env::var("FUZZ_HTML5EVER").unwrap() == "1" {
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

        did_anything = true;
    }

    if !did_anything {
        panic!("running empty testcase, enable either FUZZ_OLD_HTML5GUM or FUZZ_HTML5EVER");
    }
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
                assert_eq!(tag.name, tag2.name.as_ref());
            }
            (Some(Token::EndTag(tag)), Token2::TagToken(tag2)) => {
                assert_eq!(tag2.kind, TagKind::EndTag);
                assert_eq!(tag.name, tag2.name.as_ref());
            }
            (None, Token2::EOFToken) => {}
            (Some(Token::Comment(comment)), Token2::CommentToken(comment2)) => {
                assert_eq!(comment, comment2.as_ref());
            }
            (Some(Token::Doctype(doctype)), Token2::DoctypeToken(doctype2)) => {
                assert_eq!(
                    doctype.name,
                    doctype2
                        .name
                        .map(|x| x.as_ref().to_owned())
                        .unwrap_or_default()
                );
                assert_eq!(
                    doctype.public_identifier,
                    doctype2.public_id.map(|x| x.as_ref().to_owned())
                );
                assert_eq!(
                    doctype.system_identifier,
                    doctype2.system_id.map(|x| x.as_ref().to_owned())
                );
                assert_eq!(doctype.force_quirks, doctype2.force_quirks);
            }
            (a, b) => panic!("5gum: {:?}\n5ever: {:?}", a, b),
        }

        TokenSinkResult::Continue
    }
}
