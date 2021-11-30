use std::include_str;

use codespan_reporting::{
    self,
    diagnostic::{Diagnostic, Label},
    files::SimpleFiles,
    term::{self, termcolor::Buffer},
};
use html5gum::{
    spans::{PosTracker, SpanEmitter},
    Readable, Token, Tokenizer,
};

#[test]
fn test() {
    let html = include_str!("span-tests/demo.html");

    let mut files = SimpleFiles::new();
    let file_id = files.add("test.html", html);
    let mut labels = Vec::new();

    for token in Tokenizer::new_with_emitter(
        PosTracker {
            reader: html.to_reader(),
            position: 0,
        },
        SpanEmitter::default(),
    )
    .infallible()
    {
        if let Token::StartTag(tag) = token {
            labels.push(Label::primary(file_id, tag.name_span).with_message("start tag"));
        } else if let Token::EndTag(tag) = token {
            labels.push(Label::primary(file_id, tag.name_span).with_message("end tag"));
        }
    }

    let diagnostic = Diagnostic::note().with_labels(labels);

    let mut writer = Buffer::no_color();
    let config = codespan_reporting::term::Config::default();
    term::emit(&mut writer, &config, &files, &diagnostic).unwrap();

    let actual = remove_trailing_spaces(std::str::from_utf8(writer.as_slice()).unwrap());
    let expected = include_str!("span-tests/demo.out");

    if actual != expected {
        println!(
            "EXPECTED:\n{banner}\n{expected}{banner}\n\nACTUAL OUTPUT:\n{banner}\n{actual}{banner}",
            banner = "-".repeat(30),
            expected = expected,
            actual = actual
        );
        panic!("failed");
    }
}

fn remove_trailing_spaces(text: &str) -> String {
    text.lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}
