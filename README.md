# html5gum

[![docs.rs](https://img.shields.io/docsrs/html5gum)](https://docs.rs/html5gum)
[![crates.io](https://img.shields.io/crates/l/html5gum.svg)](https://crates.io/crates/html5gum)

`html5gum` is a WHATWG-compliant HTML tokenizer.

```rust
use std::fmt::Write;
use html5gum::{Tokenizer, Token};

let html = "<title   >hello world</title>";
let mut new_html = String::new();

for token in Tokenizer::new(html).infallible() {
    match token {
        Token::StartTag(tag) => {
            write!(new_html, "<{}>", tag.name).unwrap();
        }
        Token::String(hello_world) => {
            write!(new_html, "{}", hello_world).unwrap();
        }
        Token::EndTag(tag) => {
            write!(new_html, "</{}>", tag.name).unwrap();
        }
        _ => panic!("unexpected input"),
    }
}

assert_eq!(new_html, "<title>hello world</title>");
```

It fully implements [13.2.5 of the WHATWG HTML
spec](https://html.spec.whatwg.org/#tokenization), i.e. is able to tokenize HTML documents and passes [html5lib's tokenizer
test suite](https://github.com/html5lib/html5lib-tests/tree/master/tokenizer). Most importantly it does not:

* [Implement charset detection.](https://html.spec.whatwg.org/#determining-the-character-encoding) This implementation requires all input to be
  Rust strings and therefore valid UTF-8.

* [Correct mis-nested tags](https://html.spec.whatwg.org/#an-introduction-to-error-handling-and-strange-cases-in-the-parser)

* Generally qualify as a complete HTML *parser* as per the WHATWG spec (yet).

A distinguishing feature of `html5gum` is that you can bring your own token
datastructure and hook into token creation by implementing the `Emitter` trait.
This allows you to:

* Rewrite all per-HTML-tag allocations to use a custom allocator or datastructure.

* Efficiently filter out uninteresting categories data without ever allocating
  for it. For example if any plaintext between tokens is not of interest to
  you, you can implement the respective trait methods as noop and therefore
  avoid any overhead creating plaintext tokens.

`html5gum` was created out of a need to parse HTML tag soup efficiently. Previous options were to:

* use [quick-xml](https://github.com/tafia/quick-xml/) or
  [xmlparser](https://github.com/RazrFalcon/xmlparser) with some hacks to make
  either one not choke on bad HTML. For some (rather large) set of HTML input
  this works well (particularly `quick-xml` can be configured to be very
  lenient about parsing errors) and parsing speed is stellar. But neither can
  parse all HTML.

  For my own usecase `html5gum` is about 2x slower than `quick-xml`.

* use [html5ever's own
  tokenizer](https://docs.rs/html5ever/0.25.1/html5ever/tokenizer/index.html)
  to avoid as much tree-building overhead as possible. This was functional but
  had poor performance for my own usecase (10-15x slower than `quick-xml`).

* use [lol-html](https://github.com/cloudflare/lol-html), which would probably
  perform at least as well as `html5gum`, but comes with a closure-based API
  that I didn't manage to get working for my usecase.

## Etymology

Why is this library called `html5gum`?

* G.U.M: **G**iant **U**nreadable **M**atch-statement

* \<insert "how it feels to <s>chew 5 gum</s> _parse HTML_" meme here\>

## License

Licensed under the MIT license, see [`./LICENSE`](./LICENSE).
