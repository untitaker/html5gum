use std::convert::Infallible;
use std::fs::File;
use std::io::BufReader;

use glob::glob;
use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};
use html5gum::testutils::trace_log;
use html5gum::{DefaultEmitter, Span, Token, Tokenizer};
use libtest_mimic::{Arguments, Failed, Trial};
use pretty_assertions::assert_eq;
use serde::Deserialize;

mod testutils;

#[derive(Deserialize, Clone, Debug)]
struct SpanTestCase {
    description: String,
    input: String,
    naively_switch_states: bool,
    expected_tokens: serde_json::Value,
}

#[derive(Deserialize, Clone)]
struct TestFile {
    tests: Vec<SpanTestCase>,
}

fn token_to_json(token: &Token<usize>) -> serde_json::Value {
    match token {
        Token::Doctype(d) => serde_json::json!([
            "DOCTYPE",
            String::from_utf8_lossy(&d.name),
            d.public_identifier
                .as_ref()
                .map(|x| String::from_utf8_lossy(x).into_owned()),
            d.system_identifier
                .as_ref()
                .map(|x| String::from_utf8_lossy(x).into_owned()),
            !d.force_quirks,
            d.span.start,
            d.span.end
        ]),
        Token::StartTag(st) => {
            let attrs: serde_json::Map<String, serde_json::Value> = st
                .attributes
                .iter()
                .map(|(k, v)| {
                    (
                        String::from_utf8_lossy(k).into_owned(),
                        serde_json::json!([
                            String::from_utf8_lossy(&v.value),
                            v.span.start,
                            v.span.end
                        ]),
                    )
                })
                .collect();
            serde_json::json!([
                "StartTag",
                String::from_utf8_lossy(&st.name),
                attrs,
                st.self_closing,
                st.span.start,
                st.span.end
            ])
        }
        Token::EndTag(et) => serde_json::json!([
            "EndTag",
            String::from_utf8_lossy(&et.name),
            et.span.start,
            et.span.end
        ]),
        Token::String(s) => serde_json::json!([
            "Character",
            String::from_utf8_lossy(&s.value),
            s.span.start,
            s.span.end
        ]),
        Token::Comment(c) => serde_json::json!([
            "Comment",
            String::from_utf8_lossy(&c.value),
            c.span.start,
            c.span.end
        ]),
        Token::Error(e) => {
            serde_json::json!(["Error", format!("{:?}", e.value), e.span.start, e.span.end])
        }
    }
}

fn run_test(test: &SpanTestCase) -> Result<(), Failed> {
    testutils::catch_unwind_and_report(move || {
        trace_log(&format!("==== SPAN TEST: {} ====", test.description));
        trace_log(&format!("input: {:?}", test.input));
        trace_log(&format!(
            "naively_switch_states: {}",
            test.naively_switch_states
        ));

        let mut emitter = DefaultEmitter::new_with_span();
        emitter.naively_switch_states(test.naively_switch_states);

        let actual_tokens: Vec<Token<usize>> = Tokenizer::new_with_emitter(&test.input, emitter)
            .filter_map(|t| t.ok())
            .collect();

        let actual_json: Vec<serde_json::Value> = actual_tokens.iter().map(token_to_json).collect();

        let expected_str = if let serde_json::Value::Array(ref arr) = test.expected_tokens {
            arr.iter()
                .map(|v| serde_json::to_string(v).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            serde_json::to_string(&test.expected_tokens).unwrap()
        };

        let actual_str = actual_json
            .iter()
            .map(|v| serde_json::to_string(v).unwrap())
            .collect::<Vec<_>>()
            .join("\n");

        assert_eq!(actual_str, expected_str);
    })
}

/// The data-driven tests go through `DefaultEmitter`, whose attribute spans start at the attribute
/// name. Where the attribute *value* starts is only observable through `CallbackEmitter`.
fn test_callback_attribute_value_spans() -> Result<(), Failed> {
    testutils::catch_unwind_and_report(|| {
        let input = r#"<a b=xyz c="q" d='r' e>"#;
        let mut values = Vec::new();
        let emitter = CallbackEmitter::new(|event: CallbackEvent<'_>, span: Span<usize>| {
            if let CallbackEvent::AttributeValue { value } = event {
                values.push((String::from_utf8_lossy(value).into_owned(), span));
            }
            None::<Infallible>
        });
        Tokenizer::new_with_emitter(input, emitter)
            .finish()
            .unwrap();

        assert_eq!(
            values,
            [
                ("xyz".to_owned(), Span { start: 5, end: 8 }),
                ("q".to_owned(), Span { start: 12, end: 13 }),
                ("r".to_owned(), Span { start: 18, end: 19 }),
            ]
        );
    })
}

fn produce_tests_from_file(path: &str) -> Vec<Trial> {
    let file = File::open(path).expect("failed to open test file");
    let reader = BufReader::new(file);
    let test_file: TestFile = serde_json::from_reader(reader).expect("failed to parse test file");

    test_file
        .tests
        .into_iter()
        .map(|test| {
            let description = test.description.clone();
            Trial::test(description, move || run_test(&test))
        })
        .collect()
}

fn main() {
    let args = Arguments::from_args();
    let mut tests = Vec::new();

    for entry in glob("tests/test_spans_data/*.test").expect("failed to read glob pattern") {
        match entry {
            Ok(path) => {
                tests.extend(produce_tests_from_file(path.to_str().unwrap()));
            }
            Err(e) => eprintln!("Error reading path: {:?}", e),
        }
    }

    tests.push(Trial::test(
        "callback emitter: attribute value spans",
        test_callback_attribute_value_spans,
    ));

    libtest_mimic::run(&args, tests).exit();
}
