//! The regular `tokenize` example will see an `<a>` tag inside of this HTML:
//!
//! ```html
//! <script>
//! <a href="/hello.html">hello</a>
//! </script>
//! ```
//!
//! This is how a HTML tokenizer is supposed to work according to the WHATWG spec, because in
//! browsers, there is a feedback loop between tokenization and DOM tree building that switches the
//! parser into different modes upon detecting certain states in the tree.  `html5gum` does not
//! come with a complete DOM tree builder, but approximates this behavior by switching parsing
//! modes based on the name of the start tag. This is _not_ spec-compliant behavior and should not
//! ever be used outside of simple scraping applications, but approximates the typically-desired
//! behavior for many usecases.
//!
//! See [issue 11](https://github.com/untitaker/html5gum/issues/11) for some discussion.
use html5gum::{DefaultEmitter, IoReader, Tokenizer};

fn main() {
    let mut emitter = DefaultEmitter::default();
    emitter.naively_switch_states(true);

    let reader = IoReader::new(std::io::stdin().lock());

    for token in Tokenizer::new_with_emitter(reader, emitter).flatten() {
        println!("{:?}", token);
    }
}
