use std::collections::BTreeMap;

use swc_common::{input::StringInput, BytePos};
use swc_html_ast::*;
use swc_html_parser::lexer::Lexer;

use pretty_assertions::assert_eq;

pub fn run_swc(s: &str) {
    if s.starts_with("\u{feff}") {
        // ignore any tests with leading BOM
        return;
    }

    let lexer_str_input = StringInput::new(s, BytePos(0), BytePos(s.len() as u32));
    let mut lexer = Lexer::new(lexer_str_input);

    let mut swc_tokens = vec![];

    #[allow(clippy::while_let_on_iterator)]
    while let Some(token_and_span) = lexer.next() {
        swc_tokens.push(token_and_span.token.clone());
    }

    let mut transformed_swc_tokens = vec![];

    for token in swc_tokens {
        match token {
            Token::Doctype {
                name,
                force_quirks,
                public_id,
                system_id,
                ..
            } => {
                transformed_swc_tokens.push(html5gum::Token::Doctype(
                    html5gum::Doctype {
                        name: name.unwrap_or_default().to_string().into_bytes().into(),
                        public_identifier: public_id.map(|x| x.to_string().into_bytes().into()),
                        system_identifier: system_id.map(|x| x.to_string().into_bytes().into()),
                        force_quirks,
                    }.into(),
                ));
            }
            Token::StartTag {
                tag_name,
                is_self_closing,
                attributes,
                ..
            } => {
                transformed_swc_tokens.push(html5gum::Token::StartTag(html5gum::StartTag {
                    self_closing: is_self_closing,
                    name: tag_name.to_string().into_bytes().into(),
                    attributes: {
                        let mut gum_attributes = BTreeMap::new();
                        for token in attributes {
                            gum_attributes
                                .entry(token.name.to_string().into_bytes().into())
                                .or_insert(
                                    token
                                        .value
                                        .unwrap_or_default()
                                        .to_string()
                                        .into_bytes()
                                        .into(),
                                );
                        }

                        gum_attributes
                    },
                    ..Default::default()
                }));
            }
            Token::EndTag { tag_name, .. } => {
                transformed_swc_tokens.push(html5gum::Token::EndTag(html5gum::EndTag {
                    name: tag_name.to_string().into_bytes().into(),
                    ..Default::default()
                }));
            }
            Token::Comment { data, .. } => {
                transformed_swc_tokens.push(html5gum::Token::Comment(data.to_string().into_bytes().into()));
            }
            Token::Character { value, .. } => {
                let value_bytes = value.to_string().into_bytes();
                if let Some(html5gum::Token::String(gum_data)) = transformed_swc_tokens.last_mut() {
                    gum_data.extend(value_bytes);
                } else {
                    transformed_swc_tokens.push(html5gum::Token::String(value_bytes.into()));
                }
            }
            Token::Eof => {}
        }
    }

    let mut gum_tokens = vec![];
    for Ok(token) in html5gum::Tokenizer::new(s) {
        match token {
            html5gum::Token::Error(_) => {}
            token => gum_tokens.push(token),
        }
    }

    assert_eq!(transformed_swc_tokens, gum_tokens);
}
