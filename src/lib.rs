#![warn(missing_docs)]
// This is an HTML parser. HTML can be untrusted input from the internet.
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

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
mod slow_reader;

#[cfg(feature = "integration-tests")]
pub use {slow_reader::SlowReader, utils::State};

pub use emitter::{DefaultEmitter, Doctype, Emitter, EndTag, StartTag, Token};
pub use error::Error;
pub use never::Never;
pub use reader::{BufReadReader, Readable, Reader, StringReader};
pub use tokenizer::{InfallibleTokenizer, Tokenizer};
