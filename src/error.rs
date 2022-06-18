macro_rules! impl_error {
    ($(
        $string:literal <=> $variant:ident,
    )*) => {
        /// All [parsing errors](https://html.spec.whatwg.org/#parse-errors) this tokenizer can emit.
        #[derive(Debug, Eq, PartialEq, Clone, Copy)]
        pub enum Error {
            $(
                #[doc = "This error corresponds to the `$literal` error found in the WHATWG spec."]
                $variant
            ),*
        }
        impl std::str::FromStr for Error {
            type Err = ();

            /// Parse a `kebap-case` error code as typically written in the WHATWG spec into an
            /// enum variant.
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $( $string => Ok(Self::$variant), )*
                    _ => Err(())
                }
            }
        }

        impl Error {
            /// Convert an enum variant back into the `kebap-case` error code as typically written
            /// in the WHATWG spec.
            #[must_use]
            pub fn as_str(&self) -> &'static str {
                match *self {
                    $( Self::$variant => $string, )*
                }
            }
        }
    }
}

impl std::fmt::Display for Error {
    /// Convert an enum variant back into the `kebap-case` error code as typically written
    /// in the WHATWG spec.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}

impl_error! {
    "abrupt-closing-of-empty-comment" <=> AbruptClosingOfEmptyComment,
    "abrupt-doctype-public-identifier" <=> AbruptDoctypePublicIdentifier,
    "abrupt-doctype-system-identifier" <=> AbruptDoctypeSystemIdentifier,
    "absence-of-digits-in-numeric-character-reference" <=> AbsenceOfDigitsInNumericCharacterReference,
    "cdata-in-html-content" <=> CdataInHtmlContent,
    "character-reference-outside-unicode-range" <=> CharacterReferenceOutsideUnicodeRange,
    "control-character-reference" <=> ControlCharacterReference,
    "end-tag-with-attributes" <=> EndTagWithAttributes,
    "end-tag-with-trailing-solidus" <=> EndTagWithTrailingSolidus,
    "eof-before-tag-name" <=> EofBeforeTagName,
    "eof-in-cdata" <=> EofInCdata,
    "eof-in-comment" <=> EofInComment,
    "eof-in-doctype" <=> EofInDoctype,
    "eof-in-script-html-comment-like-text" <=> EofInScriptHtmlCommentLikeText,
    "eof-in-tag" <=> EofInTag,
    "incorrectly-closed-comment" <=> IncorrectlyClosedComment,
    "incorrectly-opened-comment" <=> IncorrectlyOpenedComment,
    "invalid-character-sequence-after-doctype-name" <=> InvalidCharacterSequenceAfterDoctypeName,
    "invalid-first-character-of-tag-name" <=> InvalidFirstCharacterOfTagName,
    "missing-attribute-value" <=> MissingAttributeValue,
    "missing-doctype-name" <=> MissingDoctypeName,
    "missing-doctype-public-identifier" <=> MissingDoctypePublicIdentifier,
    "missing-doctype-system-identifier" <=> MissingDoctypeSystemIdentifier,
    "missing-end-tag-name" <=> MissingEndTagName,
    "missing-quote-before-doctype-public-identifier" <=> MissingQuoteBeforeDoctypePublicIdentifier,
    "missing-quote-before-doctype-system-identifier" <=> MissingQuoteBeforeDoctypeSystemIdentifier,
    "missing-semicolon-after-character-reference" <=> MissingSemicolonAfterCharacterReference,
    "missing-whitespace-after-doctype-public-keyword" <=> MissingWhitespaceAfterDoctypePublicKeyword,
    "missing-whitespace-after-doctype-system-keyword" <=> MissingWhitespaceAfterDoctypeSystemKeyword,
    "missing-whitespace-before-doctype-name" <=> MissingWhitespaceBeforeDoctypeName,
    "missing-whitespace-between-attributes" <=> MissingWhitespaceBetweenAttributes,
    "missing-whitespace-between-doctype-public-and-system-identifiers" <=> MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers,
    "nested-comment" <=> NestedComment,
    "noncharacter-character-reference" <=> NoncharacterCharacterReference,
    "noncharacter-in-input-stream" <=> NoncharacterInInputStream,
    "null-character-reference" <=> NullCharacterReference,
    "surrogate-character-reference" <=> SurrogateCharacterReference,
    "surrogate-in-input-stream" <=> SurrogateInInputStream,
    "unexpected-character-after-doctype-system-identifier" <=> UnexpectedCharacterAfterDoctypeSystemIdentifier,
    "unexpected-character-in-attribute-name" <=> UnexpectedCharacterInAttributeName,
    "unexpected-character-in-unquoted-attribute-value" <=> UnexpectedCharacterInUnquotedAttributeValue,
    "unexpected-equals-sign-before-attribute-name" <=> UnexpectedEqualsSignBeforeAttributeName,
    "unexpected-null-character" <=> UnexpectedNullCharacter,
    "unexpected-question-mark-instead-of-tag-name" <=> UnexpectedQuestionMarkInsteadOfTagName,
    "unexpected-solidus-in-tag" <=> UnexpectedSolidusInTag,
    "unknown-named-character-reference" <=> UnknownNamedCharacterReference,
    "duplicate-attribute" <=> DuplicateAttribute,
    "control-character-in-input-stream" <=> ControlCharacterInInputStream,
}
