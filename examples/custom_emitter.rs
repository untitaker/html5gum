//! An example of using a custom emitter to only extract tags you care about, efficiently.
//!
//! A naive attempt at link extraction would be to tweak `examples/tokenize.rs` to just not print
//! irrelevant data.
//!
//! The approach in this example however is a lot more performant.
//!
//! With the `LinkExtractor` emitter, "Hello world!" will not be allocated individually at all (as
//! there is no `html5gum::Token::String` we have to construct), instead it will only be borrowed
//! from the input data (or I/O buffer)
//!
//! This example can be further optimized by printing directly from within the emitter impl, and
//! changing `pop_token` to always return None.
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
use html5gum::{Emitter, Error, IoReader, State, Tokenizer};

#[derive(Default)]
struct LinkExtractor {
    current_tag_name: Vec<u8>,
    current_tag_is_closing: bool,
    current_attribute_name: Vec<u8>,
    current_attribute_value: Vec<u8>,
    last_start_tag: Vec<u8>,
    queued_token: Option<String>,
}

impl LinkExtractor {
    fn flush_old_attribute(&mut self) {
        if self.current_tag_name == b"a"
            && self.current_attribute_name == b"href"
            && !self.current_attribute_value.is_empty()
            && !self.current_tag_is_closing
        {
            self.queued_token =
                Some(String::from_utf8(self.current_attribute_value.clone()).unwrap());
        }

        self.current_attribute_name.clear();
        self.current_attribute_value.clear();
    }
}

impl Emitter for LinkExtractor {
    type Token = String;

    fn set_last_start_tag(&mut self, last_start_tag: Option<&[u8]>) {
        self.last_start_tag.clear();
        self.last_start_tag
            .extend(last_start_tag.unwrap_or_default());
    }

    fn pop_token(&mut self) -> Option<String> {
        self.queued_token.take()
    }

    fn emit_string(&mut self, _: &[u8]) {}

    fn init_start_tag(&mut self) {
        self.current_tag_name.clear();
        self.current_tag_is_closing = false;
    }

    fn init_end_tag(&mut self) {
        self.current_tag_name.clear();
        self.current_tag_is_closing = true;
    }

    fn emit_current_tag(&mut self) -> Option<State> {
        self.flush_old_attribute();
        self.last_start_tag.clear();
        if !self.current_tag_is_closing {
            self.last_start_tag.extend(&self.current_tag_name);
        }
        self.current_tag_name.clear();
        html5gum::naive_next_state(&self.last_start_tag)
    }

    fn set_self_closing(&mut self) {}
    fn push_tag_name(&mut self, s: &[u8]) {
        self.current_tag_name.extend(s);
    }

    fn init_attribute(&mut self) {
        self.flush_old_attribute();
    }

    fn push_attribute_name(&mut self, s: &[u8]) {
        self.current_attribute_name.extend(s);
    }

    fn push_attribute_value(&mut self, s: &[u8]) {
        self.current_attribute_value.extend(s);
    }

    fn current_is_appropriate_end_tag_token(&mut self) -> bool {
        self.current_tag_is_closing
            && !self.current_tag_name.is_empty()
            && self.current_tag_name == self.last_start_tag
    }

    fn emit_current_comment(&mut self) {}
    fn emit_current_doctype(&mut self) {}
    fn emit_eof(&mut self) {}
    fn emit_error(&mut self, _: Error) {}
    fn init_comment(&mut self) {}
    fn init_doctype(&mut self) {}
    fn push_comment(&mut self, _: &[u8]) {}
    fn push_doctype_name(&mut self, _: &[u8]) {}
    fn push_doctype_public_identifier(&mut self, _: &[u8]) {}
    fn push_doctype_system_identifier(&mut self, _: &[u8]) {}
    fn set_doctype_public_identifier(&mut self, _: &[u8]) {}
    fn set_doctype_system_identifier(&mut self, _: &[u8]) {}
    fn set_force_quirks(&mut self) {}
}

fn main() {
    for token in Tokenizer::new_with_emitter(
        IoReader::new(std::io::stdin().lock()),
        LinkExtractor::default(),
    )
    .flatten()
    {
        println!("link: {}", token);
    }
}

#[test]
fn basic() {
    let tokens: Vec<_> = Tokenizer::new_with_emitter(
        "<h1>Hello world</h1><a href=foo>bar</a>",
        LinkExtractor::default(),
    )
    .flatten()
    .collect();

    assert_eq!(tokens, vec!["foo".to_owned()]);
}
