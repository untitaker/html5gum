fn main() {
    afl::fuzz!(|data: &[u8]| {
        if let Ok(s) = std::str::from_utf8(data) {
            for token in html5gum::Tokenizer::new(s).infallible() {
            }
        }
    });
}
