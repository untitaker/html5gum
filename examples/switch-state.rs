//! Let's you easily try out the tokenizer with e.g.
//! printf '<style><b>Hello world!</b></style>' | cargo run --example=switch-state
use html5gum::{BufReadReader, Token, Tokenizer};
use std::io::stdin;

fn main() {
    let stdin = stdin();
    let mut tokenizer = Tokenizer::new(BufReadReader::new(stdin.lock()));

    while let Some(token) = tokenizer.next() {
        let token = token.unwrap();
        println!("{:?}", token);

        if let Token::StartTag(start_tag) = token {
            // take care of switching parser state for e.g. <script> & <style>
            tokenizer.set_state(start_tag.next_state(false));
        }
    }
}
