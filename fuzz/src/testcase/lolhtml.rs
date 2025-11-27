use html5gum::{Doctype, EndTag, StartTag, Token};
use lol_html::errors::RewritingError;
use lol_html::html_content::DocumentEnd;
use lol_html::{
    AsciiCompatibleEncoding, LocalName, Namespace, SharedEncoding, SharedMemoryLimiter,
    StartTagHandlingResult, Token as Token2, TokenCaptureFlags, TransformController,
    TransformStream, TransformStreamSettings,
};

use pretty_assertions::assert_eq;

pub fn run_lolhtml(data: &[u8]) {
    if std::str::from_utf8(data).is_err() {
        // invalid utf8 is an entirely different rabbithole which is probably not worth exploring,
        // for now.
        return;
    }

    if data.contains(&b'&') {
        // do not test anything related to entity-encoding, as lol-html simply doesn't do that
        return;
    }

    if data.contains(&b'\r') {
        // do not test anything related to newline normalization, as lol-html simply doesn't do that
        return;
    }

    if data.contains(&b'\0') {
        // do not test anything related to nullbytes, as lol-html simply doesn't do that
        return;
    }

    let mut lol_tokens = Vec::new();

    {
        let transform_controller = TestTransformController {
            testing_tokenizer: &mut lol_tokens,
        };

        let memory_limiter = SharedMemoryLimiter::new(2048);

        let mut transform_stream = TransformStream::new(TransformStreamSettings {
            transform_controller,
            output_sink: |_: &[u8]| (),
            preallocated_parsing_buffer_size: 0,
            memory_limiter,
            encoding: SharedEncoding::new(
                AsciiCompatibleEncoding::new(encoding_rs::UTF_8).unwrap(),
            ),
            // we need the dumb, insecure behavior of lolhtml to match what a tokenizer does
            strict: false,
        });

        transform_stream.write(data).unwrap();
        transform_stream.end().unwrap();
    }

    let mut gum_tokens = Vec::new();
    for Ok(mut token) in html5gum::Tokenizer::new(data) {
        match token {
            Token::Error(_) => continue,
            Token::StartTag(ref mut s) => {
                s.attributes.clear();
            }
            _ => (),
        }

        gum_tokens.push(token);
    }

    assert_eq!(gum_tokens, lol_tokens);
}

struct TestTransformController<'a> {
    testing_tokenizer: &'a mut Vec<Token>,
}

const TOKEN_CAPTURE_FLAGS: TokenCaptureFlags = TokenCaptureFlags::all();

impl<'a> TransformController for TestTransformController<'a> {
    fn initial_capture_flags(&self) -> TokenCaptureFlags {
        TOKEN_CAPTURE_FLAGS
    }

    fn handle_start_tag(&mut self, _: LocalName, _: Namespace) -> StartTagHandlingResult<Self> {
        Ok(TOKEN_CAPTURE_FLAGS)
    }

    fn handle_end_tag(&mut self, _: LocalName) -> TokenCaptureFlags {
        TOKEN_CAPTURE_FLAGS
    }

    fn handle_token(&mut self, reference_token: &mut Token2) -> Result<(), RewritingError> {
        match reference_token {
            Token2::TextChunk(s) => {
                let text = s.as_str().to_owned();

                if let Some(Token::String(old_s)) = self.testing_tokenizer.last_mut() {
                    old_s.extend(text.into_bytes());
                } else {
                    self.testing_tokenizer
                        .push(Token::String(text.into_bytes().into()));
                }
            }
            Token2::Comment(c) => {
                let text = c.text().as_str().to_owned();
                self.testing_tokenizer
                    .push(Token::Comment(text.into_bytes().into()));
            }
            Token2::StartTag(t) => {
                self.testing_tokenizer.push(Token::StartTag(StartTag {
                    name: t.name().into_bytes().into(),
                    self_closing: t.self_closing(),
                    // TODO
                    attributes: Default::default(),
                    ..Default::default()
                }));
            }
            Token2::EndTag(t) => {
                self.testing_tokenizer.push(Token::EndTag(EndTag {
                    name: t.name().into_bytes().into(),
                    ..Default::default()
                }));
            }
            Token2::Doctype(d) => {
                self.testing_tokenizer.push(Token::Doctype(Doctype {
                    force_quirks: d.force_quirks(),
                    name: d.name().unwrap_or_default().into_bytes().into(),
                    public_identifier: d.public_id().map(|x| x.into_bytes().into()),
                    system_identifier: d.system_id().map(|x| x.into_bytes().into()),
                }.into()));
            }
        }

        Ok(())
    }

    fn handle_end(&mut self, _: &mut DocumentEnd) -> Result<(), RewritingError> {
        Ok(())
    }

    fn should_emit_content(&self) -> bool {
        true
    }
}
