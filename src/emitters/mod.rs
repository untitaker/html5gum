//! [Emitter] is a "visitor" on the underlying token stream.
//!
//! When html5gum parses HTML, it (more specifically, the [crate::Tokenizer]) calls into emitters to keep
//! track of state and to produce output.
//!
//! Emitters can yield control to the _caller_ of the tokenizer by emitting tokens in
//! [Emitter::pop_token]. This is what powers the basic API where users just iterate over
//! [crate::Tokenizer] which is an iterator over [default::Token].
//!
//! Most performant implementations don't implement `pop_token` and instead hold internal mutable
//! state, or directly produce side effects.
//!
//! Emitters are "a way to consume parsing results." The following ways are available:
//!
//! * [default::DefaultEmitter], if you don't care about speed and only want convenience.
//! * [callback::CallbackEmitter], if you can deal with some lifetime problems in exchange for way fewer allocations.
//! * Implementing your own [Emitter] for maximum performance and maximum pain.
pub mod callback;
pub mod default;
#[cfg(feature = "html5ever")]
pub mod html5ever;

mod emitter;

pub use emitter::{naive_next_state, Emitter};
