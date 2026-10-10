use iai::{black_box, main};

use html5gum::Tokenizer;

fn pattern(pattern: &str, i: usize) {
    let s: String = black_box((0..i).map(|_| pattern).collect());
    for Ok(_) in Tokenizer::new(&s) {}
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
    (
        charref_amp_in_url_1000,
        r#"<a href="/search?q=shoes&amp;page=2&amp;sort=price">"#,
        1000
    ),
    (
        charref_nbsp_quot_1000,
        "&quot;Hello&quot;&nbsp;world&nbsp;",
        1000
    ),
    (
        charref_legacy_1000,
        r#"&copy 2024 &reg &amp <a href="?a=1&copy=2&not=3">"#,
        1000
    ),
    (
        charref_unknown_1000,
        r#"&foo; &bar <a href="/p?id=1&lang=en&sort=asc">"#,
        1000
    ),
    (charref_numeric_1000, "&#39;&#x27;&#8212;&#169;", 1000),
];
