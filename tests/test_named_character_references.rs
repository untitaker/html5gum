//! Tokenize every named character reference in the generated table, in text and in attribute
//! values, and compare the result to a straightforward implementation of the named character
//! reference state that simply tries every name.

use std::collections::BTreeMap;

use html5gum::testutils::{named_character_references, SlowReader};
use html5gum::{Error, IoReader, Readable, Reader, Span, Spanned, StartTag, Token, Tokenizer};
use pretty_assertions::assert_eq;

type Table = Vec<(String, &'static str)>;

/// Every name with and without semicolon, followed by nothing (the end of the text or attribute
/// value), an equals sign, a letter, a digit, or a letter or digit and a semicolon.
fn inputs(table: &Table) -> Vec<Vec<u8>> {
    let mut names: Vec<&str> = table
        .iter()
        .map(|(name, _)| name.strip_suffix(';').unwrap_or(name))
        .collect();
    names.sort_unstable();
    names.dedup();

    let mut inputs = Vec::new();
    for name in names {
        for semicolon in ["", ";"] {
            for suffix in ["", "=", "a", "1", "a;", "1;"] {
                inputs.push(format!("{name}{semicolon}{suffix}").into_bytes());
            }
        }
    }
    inputs
}

/// The longest name that `input` starts with.
fn longest_match<'a>(table: &'a Table, input: &[u8]) -> Option<&'a (String, &'static str)> {
    table
        .iter()
        .filter(|(name, _)| input.starts_with(name.as_bytes()))
        .max_by_key(|(name, _)| name.len())
}

/// The expected text or attribute value and errors for `&` followed by `input`, which is followed
/// by the end of the text or attribute value.
fn expected(table: &Table, input: &[u8], in_attribute: bool) -> (Vec<u8>, Vec<Error>) {
    let verbatim = [b"&", input].concat();

    match longest_match(table, input) {
        Some((name, characters)) => {
            let rest = &input[name.len()..];
            let decoded = [characters.as_bytes(), rest].concat();

            if name.ends_with(';') {
                (decoded, vec![])
            } else if in_attribute
                && matches!(rest.first(), Some(&x) if x == b'=' || x.is_ascii_alphanumeric())
            {
                // For historical reasons, this is not decoded in attribute values.
                (verbatim, vec![])
            } else {
                (
                    decoded,
                    vec![Error::MissingSemicolonAfterCharacterReference],
                )
            }
        }
        None => {
            // The ambiguous ampersand state consumes alphanumerics, and complains about a semicolon
            // after them.
            let len = input
                .iter()
                .take_while(|x| x.is_ascii_alphanumeric())
                .count();
            if input.get(len) == Some(&b';') {
                (verbatim, vec![Error::UnknownNamedCharacterReference])
            } else {
                (verbatim, vec![])
            }
        }
    }
}

fn tokenize_with<R: Reader>(tokenizer: Tokenizer<R>) -> (Vec<Token>, Vec<Error>) {
    let mut tokens = Vec::new();
    let mut errors = Vec::new();

    for token in tokenizer {
        match token.unwrap() {
            Token::Error(error) => errors.push(*error),
            token => tokens.push(token),
        }
    }

    (tokens, errors)
}

/// Tokenize `html` with readers that implement `try_read_string` and `read_until` differently.
fn tokenize(html: &[u8]) -> [(Vec<Token>, Vec<Error>); 3] {
    [
        tokenize_with(Tokenizer::new(html)),
        tokenize_with(Tokenizer::new(IoReader::new(html))),
        tokenize_with(Tokenizer::new(SlowReader(html.to_reader()))),
    ]
}

fn string_token(value: Vec<u8>) -> Token {
    Token::String(Spanned {
        value: value.into(),
        span: Span::DUMMY,
    })
}

fn start_tag_token(attribute_value: Vec<u8>) -> Token {
    let attribute = Spanned {
        value: attribute_value.into(),
        span: Span::DUMMY,
    };

    Token::StartTag(StartTag {
        self_closing: false,
        name: b"a".to_vec().into(),
        attributes: BTreeMap::from([(b"b".to_vec().into(), attribute)]),
        span: Span::DUMMY,
    })
}

#[test]
fn table() {
    let table = named_character_references();
    assert_eq!(table.len(), 2231);
    assert!(table.contains(&("amp;".to_owned(), "&")));
    assert!(table.contains(&("amp".to_owned(), "&")));
    assert!(table.contains(&("CounterClockwiseContourIntegral;".to_owned(), "\u{2233}")));
}

#[test]
fn in_text() {
    let table = named_character_references();

    for input in inputs(&table) {
        let html = [b"&", &input[..]].concat();
        let (value, errors) = expected(&table, &input, false);

        for (tokens, actual_errors) in tokenize(&html) {
            let html = String::from_utf8_lossy(&html);
            assert_eq!(tokens, [string_token(value.clone())], "{html:?}");
            assert_eq!(actual_errors, errors, "{html:?}");
        }
    }
}

#[test]
fn in_double_quoted_attribute_value() {
    let table = named_character_references();

    for input in inputs(&table) {
        let html = [b"<a b=\"&", &input[..], b"\">"].concat();
        let (value, errors) = expected(&table, &input, true);

        for (tokens, actual_errors) in tokenize(&html) {
            let html = String::from_utf8_lossy(&html);
            assert_eq!(tokens, [start_tag_token(value.clone())], "{html:?}");
            assert_eq!(actual_errors, errors, "{html:?}");
        }
    }
}

#[test]
fn in_unquoted_attribute_value() {
    let table = named_character_references();

    for input in inputs(&table) {
        let html = [b"<a b=&", &input[..], b">"].concat();
        let (value, mut errors) = expected(&table, &input, true);
        if input.ends_with(b"=") {
            errors.push(Error::UnexpectedCharacterInUnquotedAttributeValue);
        }

        for (tokens, actual_errors) in tokenize(&html) {
            let html = String::from_utf8_lossy(&html);
            assert_eq!(tokens, [start_tag_token(value.clone())], "{html:?}");
            assert_eq!(actual_errors, errors, "{html:?}");
        }
    }
}

/// The tokenizer peeks at the character after the first one, which might be at the end of
/// `IoReader`'s buffer.
#[test]
fn at_io_reader_buffer_boundaries() {
    let table = named_character_references();
    let inputs: [&[u8]; 6] = [
        b"amp;",
        b"ampa",
        b"notin;",
        b"notit;",
        b"CounterClockwiseContourIntegral;",
        b"Aopf1;",
    ];

    for input in inputs {
        let (value, errors) = expected(&table, input, false);

        for padding in 0..100 {
            let padding = b"x".repeat(padding);
            let html = [&padding[..], b"&", input].concat();
            let reader = IoReader::new_with_buffer_size::<64>(&html[..]);
            let (tokens, actual_errors) = tokenize_with(Tokenizer::new(reader));

            let html = String::from_utf8_lossy(&html);
            let value = [&padding[..], &value[..]].concat();
            assert_eq!(tokens, [string_token(value)], "{html:?}");
            assert_eq!(actual_errors, errors, "{html:?}");
        }
    }
}
