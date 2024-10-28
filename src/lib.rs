#![warn(missing_docs)]
// This is an HTML parser. HTML can be untrusted input from the internet.
#![forbid(unsafe_code)]
//
// Relative links in the README.md don't work in rustdoc, so we have to override them.
#![doc = concat!("[LICENSE]: ", blob_url_prefix!(), "LICENSE")]
#![doc = concat!("[examples/tokenize_with_state_switches.rs]: ", blob_url_prefix!(), "examples/tokenize_with_state_switches.rs")]
#![doc = concat!("[examples/custom_emitter.rs]: ", blob_url_prefix!(), "examples/custom_emitter.rs")]
#![doc = include_str!("../README.md")]
//
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

macro_rules! blob_url_prefix {
    () => {
        concat!(
            "https://github.com/untitaker/html5gum/blob/",
            env!("CARGO_PKG_VERSION"),
            "/"
        )
    };
}

// miraculously makes warnings disappear as blob_url_prefix is used in #![doc]
use blob_url_prefix;

mod arrayvec;
mod callback_emitter;
mod char_validator;
mod emitter;
mod entities;
mod error;
#[cfg(feature = "html5ever")]
mod html5ever_emitter;
mod machine;
mod machine_helper;
mod read_helper;
mod reader;
mod state;
mod tokenizer;
mod utils;

#[cfg(debug_assertions)]
#[doc(hidden)]
pub mod testutils;

pub use callback_emitter::{CallbackEmitter, CallbackEvent};
pub use emitter::{
    naive_next_state, DefaultEmitter, Doctype, Emitter, EndTag, HtmlString, StartTag, Token,
};
pub use error::Error;
pub use reader::{IoReader, Readable, Reader, StringReader};
pub use state::State;
pub use tokenizer::{InfallibleTokenizer, Tokenizer};

#[cfg(feature = "html5ever")]
pub use html5ever_emitter::Html5everEmitter;
