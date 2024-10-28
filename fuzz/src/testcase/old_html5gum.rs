use std::env;

use html5gum::{Doctype, EndTag, StartTag, Token};

use pretty_assertions::assert_eq;

pub fn run_old_html5gum(s: &str) {
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
        .split(',')
    {
        match instruction {
            "" => {}
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
            html5gum_old::Token::String(x) => Token::String(Vec::from(x).into()),
            html5gum_old::Token::Comment(x) => Token::Comment(Vec::from(x).into()),
            html5gum_old::Token::StartTag(x) => Token::StartTag(StartTag {
                name: Vec::from(x.name).into(),
                attributes: x
                    .attributes
                    .into_iter()
                    .map(|(k, v)| (Vec::from(k).into(), Vec::from(v).into()))
                    .collect(),
                self_closing: x.self_closing,
            }),
            html5gum_old::Token::EndTag(x) => Token::EndTag(EndTag {
                name: Vec::from(x.name).into(),
            }),
            html5gum_old::Token::Error(x) => Token::Error(x.to_string().parse().unwrap()),
            html5gum_old::Token::Doctype(x) => Token::Doctype(Doctype {
                name: Vec::from(x.name).into(),
                force_quirks: x.force_quirks,
                public_identifier: x.public_identifier.map(|x| Vec::from(x).into()),
                system_identifier: x.system_identifier.map(|x| Vec::from(x).into())
            }),
        })
        .collect();

    assert_eq!(testing_tokens, reference_tokens);
}
