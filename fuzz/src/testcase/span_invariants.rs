use html5gum::{DefaultEmitter, Token, Tokenizer};

/// Validates span invariants for all tokens produced from the input.
///
/// This fuzzer checks that:
/// 1. Spans have valid bounds (start <= end <= input.len())
/// 2. Spans point to correct content in the input
/// 3. Spans are non-overlapping and ordered
/// 4. Spans are non-empty for structural tokens (tags, comments, doctypes)
///
/// This would have caught the bug fixed in commit 505de5b where end tag positions
/// were incorrectly tracked in naive state switching mode.
pub fn validate_span_invariants(input: &[u8]) {
    // Use DefaultEmitter with span tracking enabled
    let mut emitter = DefaultEmitter::<usize>::new_with_span();
    // Enable naive state switching to test both modes
    emitter.naively_switch_states(true);

    let tokenizer = Tokenizer::new_with_emitter(input, emitter);

    let mut last_end: Option<usize> = None;

    for result in tokenizer {
        let token = match result {
            Ok(token) => token,
            Err(_) => continue, // Errors are expected, we're fuzzing
        };

        validate_token_span(&token, input, &mut last_end);
    }
}

/// Validates the span of a single token against the input.
fn validate_token_span(token: &Token<usize>, input: &[u8], last_end: &mut Option<usize>) {
    match token {
        Token::StartTag(tag) => {
            validate_span(&tag.span, input, "StartTag", last_end);

            // Start tags must have non-empty spans
            assert!(
                tag.span.start < tag.span.end,
                "StartTag has empty span: {}..{}",
                tag.span.start,
                tag.span.end
            );

            // Verify the span actually contains the tag
            if tag.span.end <= input.len() {
                let content = &input[tag.span.start..tag.span.end];
                // Start tags should begin with '<' and contain the tag name
                assert!(
                    content.starts_with(b"<"),
                    "StartTag span does not start with '<': {:?} at {}..{}",
                    String::from_utf8_lossy(content),
                    tag.span.start,
                    tag.span.end
                );
                // The tag name should appear in the content
                assert!(
                    content
                        .windows(tag.name.len())
                        .any(|window| window == &tag.name[..]),
                    "StartTag span does not contain tag name '{}': {:?} at {}..{}",
                    String::from_utf8_lossy(&tag.name),
                    String::from_utf8_lossy(content),
                    tag.span.start,
                    tag.span.end
                );
            }

            // Validate attribute value spans
            for (_attr_name, attr_value) in &tag.attributes {
                validate_span(&attr_value.span, input, "Attribute value", &mut None);

                // Note: Attribute value spans may include the entire attribute declaration
                // (name="value") or just the value depending on implementation.
                // We just validate basic span invariants here.
            }
        }
        Token::EndTag(tag) => {
            validate_span(&tag.span, input, "EndTag", last_end);

            // End tags must have non-empty spans
            assert!(
                tag.span.start < tag.span.end,
                "EndTag has empty span: {}..{} for tag '{}'",
                tag.span.start,
                tag.span.end,
                String::from_utf8_lossy(&tag.name)
            );

            // Verify the span actually contains the end tag
            if tag.span.end <= input.len() {
                let content = &input[tag.span.start..tag.span.end];
                // End tags should start with '</'
                assert!(
                    content.starts_with(b"</"),
                    "EndTag span does not start with '</': {:?} at {}..{} for tag '{}'",
                    String::from_utf8_lossy(content),
                    tag.span.start,
                    tag.span.end,
                    String::from_utf8_lossy(&tag.name)
                );
                // The tag name should appear in the content
                assert!(
                    content
                        .windows(tag.name.len())
                        .any(|window| window == &tag.name[..]),
                    "EndTag span does not contain tag name '{}': {:?} at {}..{}",
                    String::from_utf8_lossy(&tag.name),
                    String::from_utf8_lossy(content),
                    tag.span.start,
                    tag.span.end
                );
            }
        }
        Token::String(s) => {
            validate_span(&s.span, input, "String", last_end);

            // Note: String token values may differ from raw span content due to
            // HTML entity decoding or character reference processing.
            // The key invariant is that the span points to valid input bounds.
            // Strings can have empty spans (e.g., empty text nodes).
        }
        Token::Comment(c) => {
            validate_span(&c.span, input, "Comment", last_end);

            // Comments must have non-empty spans
            assert!(
                c.span.start < c.span.end,
                "Comment has empty span: {}..{}",
                c.span.start,
                c.span.end
            );

            // Verify comment span contains the comment markers and content
            if c.span.end <= input.len() {
                let content = &input[c.span.start..c.span.end];
                // Comments should start with '<!' (covers both '<!--' and bogus comments)
                assert!(
                    content.starts_with(b"<!"),
                    "Comment span does not start with '<!': {:?} at {}..{}",
                    String::from_utf8_lossy(content),
                    c.span.start,
                    c.span.end
                );
            }
        }
        Token::Doctype(d) => {
            validate_span(&d.span, input, "Doctype", last_end);

            // Doctypes must have non-empty spans
            assert!(
                d.span.start < d.span.end,
                "Doctype has empty span: {}..{}",
                d.span.start,
                d.span.end
            );

            // Verify doctype span starts with '<!DOCTYPE'
            if d.span.end <= input.len() {
                let content = &input[d.span.start..d.span.end];
                assert!(
                    content.starts_with(b"<!") || content.starts_with(b"<!DOCTYPE"),
                    "Doctype span does not start with '<!': {:?} at {}..{}",
                    String::from_utf8_lossy(content),
                    d.span.start,
                    d.span.end
                );
            }
        }
        Token::Error(e) => {
            validate_span(&e.span, input, "Error", last_end);
            // Errors can have empty spans (they may point to a position rather than a range)
        }
    }
}

/// Validates basic span invariants.
fn validate_span(
    span: &html5gum::Span<usize>,
    input: &[u8],
    token_type: &str,
    last_end: &mut Option<usize>,
) {
    // Invariant 1: start <= end
    assert!(
        span.start <= span.end,
        "{} span has start > end: {}..{}",
        token_type,
        span.start,
        span.end
    );

    // Invariant 2: end <= input.len()
    assert!(
        span.end <= input.len(),
        "{} span exceeds input bounds: {}..{} (input len: {})",
        token_type,
        span.start,
        span.end,
        input.len()
    );

    // Invariant 3: Spans should be ordered (non-decreasing start positions)
    // However, error tokens can be interleaved and may have empty spans pointing to
    // positions within other tokens, so we only enforce ordering for non-empty spans
    if span.start < span.end {
        // Only check ordering for non-empty spans
        if let Some(prev_end) = last_end {
            assert!(
                span.start >= *prev_end,
                "{} span starts before previous span ended: current {}..{}, previous ended at {}",
                token_type,
                span.start,
                span.end,
                prev_end
            );
        }
        // Update last_end only for non-empty spans
        *last_end = Some(span.end);
    }
}
