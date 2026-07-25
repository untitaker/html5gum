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
use html5gum::emitters::builder::sink;
use html5gum::{Emitter, IoReader, Tokenizer};

#[derive(Default)]
struct State {
    is_anchor_tag: bool,
    link: Option<String>,
}

fn get_emitter() -> impl Emitter<Token = String> {
    sink(State::default())
        .on_tag_open(|st, name, _span| {
            st.is_anchor_tag = name == b"a";
        })
        .on_attribute(|st, _tag_name, name, value, _spans| {
            if st.is_anchor_tag && name == b"href" {
                st.link = Some(String::from_utf8_lossy(value).into_owned());
            }
        })
        .on_pop_token(|st| st.link.take())
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
