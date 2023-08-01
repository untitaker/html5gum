#![warn(missing_docs)]
// This is an HTML parser. HTML can be untrusted input from the internet.
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]
#![warn(clippy::all)]
#![warn(
    absolute_paths_not_starting_with_crate,
    rustdoc::invalid_html_tags,
    missing_copy_implementations,
    missing_debug_implementations,
    semicolon_in_expressions_from_macros,
    unreachable_pub,
    unused_extern_crates,
    variant_size_differences
)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]

mod arrayvec;
mod char_validator;
mod emitter;
mod entities;
mod error;
mod machine;
mod machine_helper;
mod read_helper;
mod reader;
mod state;
mod tokenizer;
mod utils;

#[cfg(debug_assertions)]
pub mod testutils;

pub use emitter::{
    naive_next_state, DefaultEmitter, Doctype, Emitter, EndTag, HtmlString, StartTag, Token,
};
pub use error::Error;
pub use reader::{IoReader, Readable, Reader, StringReader};
pub use state::State;
pub use tokenizer::{InfallibleTokenizer, Tokenizer};
