//! Let's you easily try out the tokenizer with e.g.
//! printf '<h1>Hello world!</h1>' | cargo run --example=tokenize
use html5gum::{IoReader, Tokenizer};

fn main() {
    for token in Tokenizer::new(IoReader::new(std::io::stdin().lock())).flatten() {
        println!("{:?}", token);
    }
}
