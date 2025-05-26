use std::cell::RefCell;
use std::convert::Infallible;
use std::rc::Rc;

use annotate_snippets::{Level, Message, Renderer, Snippet};
use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};
use html5gum::{
    DefaultEmitter, Emitter, Reader, Span, SpanBoundFromReader, SpanReader, StringReader, Token,
    Tokenizer,
};

#[allow(clippy::type_complexity)]
#[derive(Clone)]
struct MessageCollector<'a> {
    annotations: Rc<RefCell<Vec<(Level, Span<usize>, &'static str)>>>,
    input: &'a str,
}

impl<'a> MessageCollector<'a> {
    fn add(&mut self, level: Level, span: Span<usize>, label: &'static str) {
        self.annotations.borrow_mut().push((level, span, label))
    }

    fn build(&self) -> Message<'a> {
        Level::Error.title("test output").snippet(
            Snippet::source(self.input).annotations(
                self.annotations
                    .borrow()
                    .iter()
                    .map(|(level, span, label)| level.span(span.start..span.end).label(label)),
            ),
        )
    }
}

fn get_simple_callback_emitter<'a, R: Reader>(
    mut message: MessageCollector<'a>,
) -> impl Emitter<R, Token = Infallible> + use<'a, R>
where
    usize: SpanBoundFromReader<R>,
{
    let mut in_h1 = false;
    CallbackEmitter::new(
        move |event: CallbackEvent<'_>, span: Span<usize>, _reader: &'_ _| {
            match event {
                CallbackEvent::OpenStartTag { name } => {
                    if name == b"h1" {
                        in_h1 = true;
                        message.add(Level::Warning, span, "h1 start");
                    } else {
                        in_h1 = false;
                    }
                }
                CallbackEvent::AttributeName { name } => {
                    if name == b"attr" {
                        message.add(Level::Warning, span, "attribute name");
                    }
                }
                CallbackEvent::AttributeValue { value } => {
                    if value == b"value" {
                        message.add(Level::Warning, span, "attribute value");
                    }
                }
                CallbackEvent::CloseStartTag { self_closing } => {
                    if in_h1 || self_closing {
                        message.add(Level::Note, span, "close start tag");
                    }
                }
                CallbackEvent::EndTag { name } => {
                    if name == b"h2" {
                        message.add(Level::Info, span, "end tag");
                    }
                }
                CallbackEvent::String { value } => {
                    if value == b"content" {
                        message.add(Level::Note, span, "content");
                    }
                }
                CallbackEvent::Comment { value: _ } => {
                    message.add(Level::Info, span, "comment");
                }
                CallbackEvent::Doctype { .. } => {
                    message.add(Level::Info, span, "doctype");
                }
                CallbackEvent::Error(error) => unreachable!("error: {}", error),
            }
            None
        },
    )
}

fn get_full_callback_emitter<'a, R: Reader>(
    mut message: MessageCollector<'a>,
) -> impl Emitter<R, Token = Infallible> + use<'a, R>
where
    usize: SpanBoundFromReader<R>,
{
    CallbackEmitter::new(
        move |event: CallbackEvent<'_>, span: Span<usize>, _reader: &'_ _| {
            match event {
                CallbackEvent::OpenStartTag { name: _ } => {
                    message.add(Level::Warning, span, "open start tag");
                }
                CallbackEvent::AttributeName { name: _ } => {
                    message.add(Level::Warning, span, "attribute name");
                }
                CallbackEvent::AttributeValue { value: _ } => {
                    message.add(Level::Note, span, "attribute value");
                }
                CallbackEvent::CloseStartTag { self_closing } => {
                    if self_closing {
                        message.add(Level::Note, span, "close start tag (self closing)");
                    } else {
                        message.add(Level::Note, span, "close start tag");
                    }
                }
                CallbackEvent::EndTag { name: _ } => {
                    message.add(Level::Info, span, "end tag");
                }
                CallbackEvent::String { value } => {
                    if value != b"\n" {
                        message.add(Level::Note, span, "string");
                    }
                }
                CallbackEvent::Comment { value: _ } => {
                    message.add(Level::Info, span, "comment");
                }
                CallbackEvent::Doctype { .. } => {
                    message.add(Level::Info, span, "doctype");
                }
                CallbackEvent::Error(error) => panic!("error: {}", error),
            }
            None
        },
    )
}

fn run<E, T>(
    input: &'static str,
    expected: &'static str,
    emitter: impl FnOnce(MessageCollector<'static>) -> E,
    mut on_token: impl FnMut(T),
) where
    E: Emitter<SpanReader<StringReader<'static>>, Token = T>,
{
    let message = MessageCollector {
        annotations: Default::default(),
        input,
    };

    let emitter = emitter(message.clone());
    let reader = SpanReader::new(input);
    for token in Tokenizer::new_with_emitter(reader, emitter) {
        on_token(token.unwrap());
    }

    let got = Renderer::plain().render(message.build()).to_string();
    let got = got.trim();
    let pretty = Renderer::styled().render(message.build()).to_string();
    let pretty = pretty.trim();

    if got != expected {
        println!(
            "expected ({} chars):\n{expected}\n\ngot ({} chars):\n{got}",
            expected.len(),
            got.len()
        );
        println!("pretty:\n{pretty}");
        panic!();
    }
}

#[test]
fn callback() {
    run(
        include_str!("test_spans/input.html"),
        include_str!("test_spans/callback.stdout.txt").trim(),
        get_simple_callback_emitter,
        |_| (),
    );

    run(
        include_str!("test_spans/input.html"),
        include_str!("test_spans/callback_full.stdout.txt").trim(),
        get_full_callback_emitter,
        |_| (),
    );
}

#[test]
fn simple() {
    let msg: Rc<RefCell<Option<_>>> = Default::default();
    run(
        include_str!("test_spans/input.html"),
        include_str!("test_spans/simple.stdout.txt").trim(),
        |m| {
            *msg.borrow_mut() = Some(m);
            DefaultEmitter::new_with_span()
        },
        |token| {
            let mut msg = msg.borrow_mut();
            let msg = msg.as_mut().unwrap();
            match token {
                Token::StartTag(start_tag) => {
                    msg.add(Level::Info, start_tag.span, "start tag");
                    for (_name, value) in start_tag.attributes {
                        msg.add(Level::Note, value.span, "attribute");
                    }
                }
                Token::EndTag(end_tag) => {
                    msg.add(Level::Note, end_tag.span, "end tag");
                }
                Token::String(v) => {
                    if **v != b"\n" {
                        msg.add(Level::Info, v.span, "string");
                    }
                }
                Token::Comment(v) => {
                    msg.add(Level::Info, v.span, "comment");
                }
                Token::Doctype(v) => {
                    msg.add(Level::Info, v.span, "doctype");
                }
                Token::Error(error) => panic!("error: {}", *error),
            }
        },
    );
}
