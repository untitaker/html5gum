use crate::{Reader, Token, Tokenizer};

enum ElementNamespace {
    HTML,
    MathML,
    SVG,
    XLink,
    XML,
    XMLNS,
    Custom(String),
}

enum InsertionMode {
    Initial,
}

fn strip_prefix_chars(value: &mut Vec<u8>, cond: impl Fn(u8) -> bool) {
    let split_at_i = value
        .iter().enumerate().find(|(i, x)| !cond(x))
        .map(|(i, _)| i)
        .unwrap_or(value.len());

    value.copy_within(split_at_i.., 0);
    value.truncate(split_at_i);
}

macro_rules! skip_over_chars {
    ($token:expr, $chars:pat) => {
        if let Token::String(mut string) = $token {
            strip_prefix_chars($token, |x| matches!(x, $chars));
            if string.is_empty() {
                return;
            }
        }
    }
}

enum InsertPosition {
    DocumentLastChild,
}

impl InsertionMode {
    fn process(&self, ctor: &mut TreeConstructionDispatcher, token: Token) {
        match InsertionMode {
            InsertionMode::Initial => {
                skip_over_chars!(token, (b'\t' | b'\x0A' | b'\x0C' | b' '));
                match token {
                    Token::Comment(s) => {
                        ctor.insert_a_comment(s, InsertPosition::DocumentLastChild);
                    }
                    Token::Doctype(doctype) => {
                        if doctype.name != b"html" || doctype.public_identifier.is_some() || (doctype.system_identifier.as_ref().map_or(false, |x| x != b"about:legacy-compat")) {
                            ctor.parse_error();
                        }

                        todo!()
                    }
                }
            }
        }
    }
}

struct Element {
    namespace: Option<ElementNamespace>,
    prefix: Option<String>,
    local_name: String,
    tag_name: String,
}

impl Element {
    fn is_mathml_text_integration_point(&self) -> bool {
        matches!(self.namespace, Some(ElementNamespace::MathML))
            && (matches!(self.local_name.as_str(), "mi" | "mo" | "mn" | "ms" | "mtext"))
    }

    fn is_html_integration_point(&self) -> bool {
        todo!()
    }
}

pub struct TreeConstructionDispatcher<R: Reader> {
    tokenizer: Tokenizer<R>,
    stack_of_open_elements: Vec<Element>,
    context_element: Option<Element>,
    current_node: Option<Element>,
    insertion_mode: InsertionMode,
}

impl<R: Reader> TreeConstructionDispatcher<R> {
    pub fn new(tokenizer: Tokenizer<R>) -> Self {
        TreeConstructionDispatcher {
            tokenizer,
            stack_of_open_elements: Vec::new(),
            context_element: None,
            current_node: None,
            insertion_mode: InsertionMode::Initial
        }
    }
    fn adjusted_current_node(&self) -> &Element {
        self.context_element.as_ref().unwrap_or(&self.current_node)
    }

    fn run(mut self) -> Result<(), R::Error> {
        while let Some(token) = self.tokenizer.next() {
            self.process_token(token?);
        }

        // eof token
        self.process_token_via_insertion_mode(None);
        Ok(())
    }

    fn process_token(&mut self, token: Token) {
        if self.stack_of_open_elements.is_empty()
            || matches!(self.adjusted_current_node().namespace, Some(ElementNamespace::HTML))
            || (self
                .adjusted_current_node()
                .is_mathml_text_integration_point()
                && (matches!(token, Token::StartTag(ref tag) if !matches!(&tag.name[..], b"mglyph" | b"malignmark"))
                    || matches!(token, Token::String(_))))
            || (matches!(self.adjusted_current_node().namespace, Some(ElementNamespace::MathML))
                && self.adjusted_current_node().local_name == "annotation-xml"
                && matches!(token, Token::StartTag(ref tag) if *tag.name == b"svg"))
            || (self.adjusted_current_node().is_html_integration_point()
                && matches!(token, Token::StartTag(_) | Token::String(_)))
        {
            self.process_token_via_insertion_mode(Some(token))
        } else {
            self.process_token_via_foreign_content(token)
        }
    }

    fn process_token_via_insertion_mode(&mut self, _token: Option<Token>) {
        todo!()
    }

    fn process_token_via_foreign_content(&mut self, _token: Token) {
        todo!()
    }

    fn insert_a_comment(&mut self, comment_string: Vec<u8>, position: InsertPosition) {
        todo!()
    }
    
    fn parse_error(&mut self) {
        todo!()
    }
}
