//! Let's you easily try out the tokenizer with e.g.
//! printf '<h1>Hello world!</h1>' | cargo run --example=tokenize_with_spans
use html5gum::{DefaultEmitter, IoReader, Tokenizer};

fn main() {
    let mut emitter = DefaultEmitter::<usize>::new_with_span();
    if std::env::var("NAIVELY_SWITCH_STATES") == Ok("1".to_owned()) {
        emitter.naively_switch_states(true);
    }

    for token in
        Tokenizer::new_with_emitter(IoReader::new(std::io::stdin().lock()), emitter).flatten()
    {
        println!("{:?}", token);
    }
}
