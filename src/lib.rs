#![warn(missing_docs)]
// This is an HTML parser. HTML can be untrusted input from the internet.
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod arrayvec;
mod char_validator;
mod emitter;
mod entities;
mod error;
mod machine;
mod machine_helper;
mod never;
mod read_helper;
mod reader;
mod tokenizer;
mod utils;

#[cfg(feature = "integration-tests")]
pub mod testutils;

pub(crate) fn trace_log(_msg: String) {
    #[cfg(feature = "integration-tests")]
    testutils::trace_log(_msg);
}

pub use emitter::{DefaultEmitter, Doctype, Emitter, EndTag, StartTag, Token};
pub use error::Error;
pub use never::Never;
pub use reader::{IoReader, Readable, Reader, StringReader};
pub use tokenizer::{InfallibleTokenizer, Tokenizer};
