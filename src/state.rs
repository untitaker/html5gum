/// Tokenizer that the tokenizer can be switched to from within the emitter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    /// The data state.
    Data,
    /// The plain text state.
    PlainText,
    /// The RC data state.
    RcData,
    /// The raw text state.
    RawText,
    /// The script data state.
    ScriptData,
    /// The cdata section state.
    CdataSection,
}
