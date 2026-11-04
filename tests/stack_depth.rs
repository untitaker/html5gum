use html5gum::Tokenizer;

/// The tokenizer must not use stack space proportional to the number of attributes.
#[test]
fn many_attributes_small_stack() {
    let shapes = [
        " a=\"v\"",
        " a='v'",
        " a=v",
        " a",
        " /a=\"v\"",
        " a=\"&amp;\"",
    ];

    for shape in shapes {
        let html = format!("<p{}>", shape.repeat(100_000));
        let count = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || Tokenizer::new(&html).flatten().count())
            .unwrap()
            .join()
            .unwrap();
        assert!(count > 0, "{:?}", shape);
    }
}
