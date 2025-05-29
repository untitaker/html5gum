#![allow(non_snake_case)]
use iai::{black_box, main};

use html5gum::Tokenizer;

// files taken from https://github.com/AndreasMadsen/htmlparser-benchmark/tree/master/files
const F0050: &str = include_str!("files/0050.html");
const F6D30: &str = include_str!("files/6D30.html");

fn file(file: &str, i: usize) {
    let s: String = black_box((0..i).map(|_| file).collect());
    for Ok(_) in Tokenizer::new(&s) {}
}

macro_rules! pattern_tests {
    ($(($name:ident, $file:expr, $repeat:expr), )*) => {
        $(
            fn $name() {
                file($file, $repeat)
            }
        )*

        main!($($name),*);
    }
}

pattern_tests![
    (file_0050_10, F0050, 10),
    (file_0050_100, F0050, 100),
    (file_6D30_10, F6D30, 10),
    (file_6D30_100, F6D30, 100),
];
