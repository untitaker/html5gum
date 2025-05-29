//! A modified version of `examples/callback_emitter.rs` which prints the location of all links in
//! the input.
//!
//! ```text
//! printf '<h1>Hello world!</h1><a href="foo">bar</a>' | cargo run --example=spans
//! ```
//!
//! Output:
//!
//! ```text
//! link: foo
//! ```
use annotate_snippets::{Level, Renderer, Snippet};
use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};
use html5gum::{Emitter, IoReader, Reader, Span, SpanBoundFromReader, SpanReader, Tokenizer};

fn get_emitter<R: Reader>() -> impl Emitter<R, Token = (String, Span<usize>)>
where
    usize: SpanBoundFromReader<R>,
{
    let mut is_anchor_tag = false;
    let mut is_href_attr = false;

    CallbackEmitter::new(
        move |event: CallbackEvent<'_>, span: Span<usize>, _reader: &R| match event {
            CallbackEvent::OpenStartTag { name } => {
                is_anchor_tag = name == b"a";
                is_href_attr = false;
                None
            }
            CallbackEvent::AttributeName { name } => {
                is_href_attr = name == b"href";
                None
            }
            CallbackEvent::AttributeValue { value } if is_anchor_tag && is_href_attr => Some((
                format!(
                    "found link with content `{}` here",
                    String::from_utf8_lossy(value)
                ),
                span,
            )),
            _ => None,
        },
    )
}

struct CollectingReader<R> {
    inner: R,
    read: Vec<u8>,
}

impl<R: std::io::Read> std::io::Read for CollectingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.inner.read(buf)?;
        self.read.extend_from_slice(&buf[..len]);
        Ok(len)
    }
}

fn main() {
    let mut reader = CollectingReader {
        inner: std::io::stdin().lock(),
        read: Vec::new(),
    };
    let spans =
        Tokenizer::new_with_emitter(SpanReader::new(IoReader::new(&mut reader)), get_emitter())
            .flatten()
            .collect::<Vec<_>>();
    let source = String::from_utf8_lossy(&reader.read);
    let mut message = Level::Info.title("found link");
    for (label, span) in &spans {
        message = message.snippet(
            Snippet::source(&source)
                .origin("<stdin>")
                .fold(true)
                .annotation(Level::Info.span(span.start..span.end).label(label)),
        );
    }
    let renderer = Renderer::styled();
    println!("{}", renderer.render(message));
}
