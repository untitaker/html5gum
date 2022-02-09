use iai::{black_box, main};

use html5gum::Tokenizer;

fn pattern(pattern: &str, i: usize) {
    let s: String = black_box((0..i).map(|_| pattern).collect());
    for _ in Tokenizer::new(&s).infallible() {}
}

macro_rules! pattern_tests {
    ($(($name:ident, $pattern:expr, $repeat:expr), )*) => {
        $(
            fn $name() {
                pattern($pattern, $repeat)
            }
        )*

        main!($($name),*);
    }
}

pattern_tests![
    (data_state_10, "a", 10),
    (data_state_10000, "a", 10000),
    (tagopen_10, "<a>", 10),
    (tagopen_10000, "<a>", 10000),
    (tagopenclose_10, "<a></a>", 10),
    (tagopenclose_10000, "<a></a>", 10000),
    (comment_10, "<!-- -->", 10),
    (comment_10000, "<!-- -->", 10000),
];
