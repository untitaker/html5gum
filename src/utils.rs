macro_rules! surrogate_pat {
    () => {
        0xd800..=0xdfff
    };
}

pub(crate) use surrogate_pat;

macro_rules! noncharacter_pat {
    () => {
        0xfdd0
            ..=0xfdef
                | 0xfffe
                | 0xffff
                | 0x1fffe
                | 0x1ffff
                | 0x2fffe
                | 0x2ffff
                | 0x3fffe
                | 0x3ffff
                | 0x4fffe
                | 0x4ffff
                | 0x5fffe
                | 0x5ffff
                | 0x6fffe
                | 0x6ffff
                | 0x7fffe
                | 0x7ffff
                | 0x8fffe
                | 0x8ffff
                | 0x9fffe
                | 0x9ffff
                | 0xafffe
                | 0xaffff
                | 0xbfffe
                | 0xbffff
                | 0xcfffe
                | 0xcffff
                | 0xdfffe
                | 0xdffff
                | 0xefffe
                | 0xeffff
                | 0xffffe
                | 0xfffff
                | 0x10fffe
                | 0x10ffff
    };
}

pub(crate) use noncharacter_pat;

// When integration tests are running, this enum is public and we get warnings about missing docs.
// However, it's not actually part of public API.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Data,
    RcData,
    RawText,
    ScriptData,
    PlainText,
    TagOpen,
    EndTagOpen,
    TagName,
    RcDataLessThanSign,
    RcDataEndTagOpen,
    RcDataEndTagName,
    RawTextLessThanSign,
    RawTextEndTagOpen,
    RawTextEndTagName,
    ScriptDataLessThanSign,
    ScriptDataEndTagOpen,
    ScriptDataEndTagName,
    ScriptDataEscapeStart,
    ScriptDataEscapeStartDash,
    ScriptDataEscaped,
    ScriptDataEscapedDash,
    ScriptDataEscapedDashDash,
    ScriptDataEscapedLessThanSign,
    ScriptDataEscapedEndTagOpen,
    ScriptDataEscapedEndTagName,
    ScriptDataDoubleEscapeStart,
    ScriptDataDoubleEscaped,
    ScriptDataDoubleEscapedDash,
    ScriptDataDoubleEscapedDashDash,
    ScriptDataDoubleEscapedLessThanSign,
    ScriptDataDoubleEscapeEnd,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentLessThanSign,
    CommentLessThanSignBang,
    CommentLessThanSignBangDash,
    CommentLessThanSignBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    AfterDoctypePublicKeyword,
    BeforeDoctypePublicIdentifier,
    DoctypePublicIdentifierDoubleQuoted,
    DoctypePublicIdentifierSingleQuoted,
    AfterDoctypePublicIdentifier,
    BetweenDoctypePublicAndSystemIdentifiers,
    AfterDoctypeSystemKeyword,
    BeforeDoctypeSystemIdentifier,
    DoctypeSystemIdentifierDoubleQuoted,
    DoctypeSystemIdentifierSingleQuoted,
    AfterDoctypeSystemIdentifier,
    BogusDoctype,
    CdataSection,
    CdataSectionBracket,
    CdataSectionEnd,
    CharacterReference,
    NamedCharacterReference,
    AmbiguousAmpersand,
    NumericCharacterReference,
    HexadecimalCharacterReferenceStart,
    HexadecimalCharacterReference,
    DecimalCharacterReference,
    NumericCharacterReferenceEnd,
}

pub(crate) enum ControlToken {
    Eof,
    Continue,
}

macro_rules! ctostr {
    ($c:expr) => {
        &*$c.encode_utf8(&mut [0; 4])
    };
}

pub(crate) use ctostr;

/// Repeatedly call `f` with chunks of lowercased characters from `s`.
pub(crate) fn with_lowercase_str(s: &[u8], mut f: impl FnMut(&[u8])) {
    if s.iter().any(u8::is_ascii_uppercase) {
        for x in s {
            f(&[x.to_ascii_lowercase()]);
        }
    } else {
        f(s);
    }
}

// having this be a macro is performance critical. rustc appears to be unable to optimize away code
// like this:
//
// ```rust
// fn noop(s: &str) {}
//
// noop(&format!("foo"));
// ```
//
// format!() + its string allocation still exists in resulting code
macro_rules! trace_log {
    ($($tt:tt)*) => {{
        #[cfg(feature = "integration-tests")]
        crate::testutils::trace_log(&format!($($tt)*));
    }};
}

pub(crate) use trace_log;
