//! A slightly simpler, but less performant version of the link extractor that can be found in
//! `examples/custom_emitter.rs`.
//!
//! ```text
//! printf '<h1>Hello world!</h1><a href="foo">bar</a>' | cargo run --example=custom_emitter
//! ```
//!
//! Output:
//!
//! ```text
//! link: foo
//! ```
use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};
use html5gum::{Emitter, IoReader, Span, Tokenizer};

fn get_emitter() -> impl Emitter<Token = String> {
    let mut is_anchor_tag = false;
    let mut is_href_attr = false;

    CallbackEmitter::new(
        move |event: CallbackEvent<'_>, _span: Span<()>| match event {
            CallbackEvent::OpenStartTag { name } => {
                is_anchor_tag = name == b"a";
                is_href_attr = false;
                None
            }
            CallbackEvent::AttributeName { name } => {
                is_href_attr = name == b"href";
                None
            }
            CallbackEvent::AttributeValue { value } if is_anchor_tag && is_href_attr => {
                Some(String::from_utf8_lossy(value).into_owned())
            }
            _ => None,
        },
    )
}

fn main() {
    for token in
        Tokenizer::new_with_emitter(IoReader::new(std::io::stdin().lock()), get_emitter()).flatten()
    {
        println!("link: {}", token);
    }
}

#[test]
fn basic() {
    let tokens: Vec<_> =
        Tokenizer::new_with_emitter("<h1>Hello world</h1><a href=foo>bar</a>", get_emitter())
            .flatten()
            .collect();

    assert_eq!(tokens, vec!["foo".to_owned()]);
}

#[test]
fn round_trip() {
    let mut rt = Vec::new();
    let emitter = CallbackEmitter::new(|event: CallbackEvent<'_>, _: html5gum::Span<()>| {
        match event {
            CallbackEvent::OpenStartTag { name } => {
                rt.push(b'<');
                rt.extend(name);
            }
            CallbackEvent::AttributeName { name } => {
                rt.push(b' ');
                rt.extend(name);
            }
            CallbackEvent::AttributeValue { value } => {
                rt.push(b'=');
                rt.push(b'"');
                rt.extend(value);
                rt.push(b'"');
            }
            CallbackEvent::String { value } => {
                rt.extend(value);
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                if self_closing {
                    rt.push(b'/');
                }
                rt.push(b'>');
            }
            CallbackEvent::EndTag { name } => {
                rt.extend(b"</");
                rt.extend(name);
                rt.push(b'>');
            }
            CallbackEvent::Comment { value } => {
                rt.extend(b"<!--");
                rt.extend(value);
                rt.extend(b"-->");
            }
            CallbackEvent::Doctype { .. } => {}
            CallbackEvent::Error(_) => {}
        }

        None::<core::convert::Infallible>
    });

    let source = " <!-- a --> <h1>Hello</h1> world <a href=\"foo\" title=\"baz\">bar</a>";
    Tokenizer::new_with_emitter(source, emitter).finish();
    assert_eq!(source.as_bytes(), rt, "{} != {}", source, rt.escape_ascii());
}
