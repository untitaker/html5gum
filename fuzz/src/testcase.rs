use std::env;

use html5ever::buffer_queue::BufferQueue;
use html5ever::tendril::format_tendril;
use html5ever::tokenizer::{TagKind, Token as Token2, TokenSinkResult, TokenizerResult};
use html5gum::{Doctype, Emitter, EndTag, Reader, StartTag, Token};

use pretty_assertions::assert_eq;

pub fn run(s: &[u8]) {
    let mut did_anything = env::var("FUZZ_BASIC").unwrap() == "1";

    // unconditionally run tokenizer against raw bytes, it should never crash. unclear whether this
    // is optimized away, unlikely though.
    let testing_tokenizer = html5gum::Tokenizer::new(s).infallible();
    for _ in testing_tokenizer {}

    if env::var("FUZZ_OLD_HTML5GUM").unwrap() == "1" {
        if let Ok(data) = std::str::from_utf8(s) {
            run_old_html5gum(data);
        }

        did_anything = true;
    }

    if env::var("FUZZ_HTML5EVER").unwrap() == "1" {
        if let Ok(data) = std::str::from_utf8(s) {
            run_html5ever(data);
        }

        did_anything = true;
    }

    if !did_anything {
        panic!("running empty testcase, enable either FUZZ_OLD_HTML5GUM or FUZZ_HTML5EVER");
    }
}

fn run_old_html5gum(s: &str) {
    let reference_tokenizer = html5gum_old::Tokenizer::new(s).infallible();
    let testing_tokenizer = html5gum::Tokenizer::new(s).infallible();

    let mut testing_tokens: Vec<_> = testing_tokenizer.collect();
    let mut reference_tokens: Vec<_> = reference_tokenizer.collect();

    fn isnt_error(x: &html5gum::Token) -> bool {
        !matches!(*x, html5gum::Token::Error(_))
    }

    fn isnt_old_error(x: &html5gum_old::Token) -> bool {
        !matches!(*x, html5gum_old::Token::Error(_))
    }

    for instruction in env::var("FUZZ_IGNORE_PARSE_ERRORS")
        .unwrap()
            .as_str()
            .trim()
            .split(",")
    {
        match instruction {
            "1" => {
                testing_tokens.retain(isnt_error);
                reference_tokens.retain(isnt_old_error);
            }
            "order" => {
                testing_tokens.sort_by_key(isnt_error);
                reference_tokens.sort_by_key(isnt_old_error);
            }
            x if x.starts_with("if-reference-contains:") => {
                if reference_tokens.contains(&html5gum_old::Token::Error(
                        x["if-reference-contains:".len()..].parse().unwrap(),
                )) {
                    reference_tokens.retain(isnt_old_error);
                    testing_tokens.retain(isnt_error);
                        }
            }
            x => panic!("unknown FUZZ_IGNORE_PARSE_ERRORS instruction: {}", x),
        }
    }

    let reference_tokens: Vec<_> = reference_tokens
        .into_iter()
        .map(|x| match x {
            html5gum_old::Token::String(x) => Token::String(x.into_bytes()),
            html5gum_old::Token::Comment(x) => Token::Comment(x.into_bytes()),
            html5gum_old::Token::StartTag(x) => Token::StartTag(StartTag {
                name: x.name.into_bytes(),
                attributes: x
                    .attributes
                    .into_iter()
                    .map(|(k, v)| (k.into_bytes(), v.into_bytes()))
                    .collect(),
                    self_closing: x.self_closing,
            }),
            html5gum_old::Token::EndTag(x) => Token::EndTag(EndTag {
                name: x.name.into_bytes(),
            }),
            html5gum_old::Token::Error(x) => Token::Error(x.to_string().parse().unwrap()),
            html5gum_old::Token::Doctype(x) => Token::Doctype(Doctype {
                name: x.name.into_bytes(),
                force_quirks: x.force_quirks,
                public_identifier: x.public_identifier.map(String::into_bytes),
                system_identifier: x.system_identifier.map(String::into_bytes),
            }),
        })
    .collect();

    assert_eq!(testing_tokens, reference_tokens);
}

fn run_html5ever(s: &str) {
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
                assert_eq!(tag.name, tag2.name.as_ref().as_bytes());
            }
            (Some(Token::EndTag(tag)), Token2::TagToken(tag2)) => {
                assert_eq!(tag2.kind, TagKind::EndTag);
                assert_eq!(tag.name, tag2.name.as_ref().as_bytes());
            }
            (None, Token2::EOFToken) => {}
            (Some(Token::Comment(comment)), Token2::CommentToken(comment2)) => {
                assert_eq!(comment, comment2.as_ref().as_bytes());
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
                        .map(|x| x.as_ref().to_owned().into_bytes())
                );
                assert_eq!(
                    doctype.system_identifier,
                    doctype2
                        .system_id
                        .map(|x| x.as_ref().to_owned().into_bytes())
                );
                assert_eq!(doctype.force_quirks, doctype2.force_quirks);
            }
            (a, b) => panic!("5gum: {:?}\n5ever: {:?}", a, b),
        }

        TokenSinkResult::Continue
    }
}
