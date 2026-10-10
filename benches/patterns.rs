use iai::{black_box, main};

use html5gum::Tokenizer;

// An inline SVG icon, as found on many websites. SVG uses camelCase tag and attribute names.
const INLINE_SVG: &str = r#"<svg viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet"><linearGradient gradientUnits="userSpaceOnUse"><stop offset="0"/></linearGradient><path d="M0 0h24v24H0z"/></svg>"#;

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
    (uppercase_tagopenclose_10, "<DIV></DIV>", 10),
    (uppercase_tagopenclose_10000, "<DIV></DIV>", 10000),
    (inline_svg_10, INLINE_SVG, 10),
    (inline_svg_10000, INLINE_SVG, 10000),
];
