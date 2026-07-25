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
use html5gum::emitters::builder::spanned_sink;
use html5gum::{Emitter, IoReader, Span, Tokenizer};

#[derive(Default)]
struct State {
    is_anchor_tag: bool,
    link: Option<(String, Span<usize>)>,
}

fn get_emitter() -> impl Emitter<Token = (String, Span<usize>)> {
    spanned_sink::<usize, _>(State::default())
        .on_tag_open(|st, name, _span| {
            st.is_anchor_tag = name == b"a";
        })
        .on_attribute(|st, _tag_name, name, value, spans| {
            if st.is_anchor_tag && name == b"href" {
                st.link = Some((
                    format!(
                        "found link with content `{}` here",
                        String::from_utf8_lossy(value)
                    ),
                    spans.value,
                ));
            }
        })
        .on_pop_token(|st| st.link.take())
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
    let spans = Tokenizer::new_with_emitter(IoReader::new(&mut reader), get_emitter())
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
