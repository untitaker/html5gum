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
