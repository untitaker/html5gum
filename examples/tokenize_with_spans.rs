//! Let's you easily try out the tokenizer with e.g.
//! printf '<h1>Hello world!</h1>' | cargo run --example=tokenize_with_spans
use html5gum::{IoReader, Tokenizer, DefaultEmitter};

fn main() {
    let emitter = DefaultEmitter::<usize>::new_with_span();
    for token in Tokenizer::new_with_emitter(IoReader::new(std::io::stdin().lock()), emitter).flatten() {
        println!("{:?}", token);
    }
}
