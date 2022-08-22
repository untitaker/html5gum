use crate::{Reader, Token, Tokenizer, HtmlString};

#[derive(Clone)]
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
    BeforeHtml,
    BeforeHead,
}

fn strip_prefix_chars(value: &mut Vec<u8>, cond: impl Fn(u8) -> bool) {
    let split_at_i = value
        .iter().enumerate().find(|(_, x)| !cond(**x))
        .map(|(i, _)| i)
        .unwrap_or(value.len());

    value.copy_within(split_at_i.., 0);
    value.truncate(split_at_i);
}

macro_rules! skip_over_chars {
    ($token:expr, $($chars:pat)|*) => {
        if let Some(Token::String(ref mut string)) = $token {
            strip_prefix_chars(&mut *string, |x| matches!(x, $($chars)|*));
            if string.is_empty() {
                return;
            }
        }
    }
}

enum InsertPosition {
    DocumentLastChild,
}

#[derive(Default, Clone)]
struct Document {
    quirks_mode: bool,
    limited_quirks_mode: bool,
    parser_cannot_change_the_mode: bool,
    nodes: Vec<Node>,
    srcdoc: Option<HtmlString>,
}

#[derive(Clone)]
struct Doctype {
    name: HtmlString,
    public_identifier: Option<HtmlString>,
    system_identifier: Option<HtmlString>,
}

#[derive(Clone)]
struct Node {
    node_document: Option<Document>,
    inner: NodeInner,
}

#[derive(Clone)]
enum NodeInner {
    Element(Element),
    Doctype(Doctype),
    Document(Document),
}

impl Node {
    fn element(element: Element) -> Self {
        Node {
            node_document: None,
            inner: NodeInner::Element(element)
        }
    }

    fn document(document: Document) -> Self {
        Node{ node_document: None, inner: NodeInner::Document(document) }
    }

    fn doctype(doctype: Doctype) -> Self {
        Node{ node_document: None, inner: NodeInner::Doctype(doctype) }
    }

    fn as_element(&self) -> Option<&Element> {
        match self.inner {
            NodeInner::Element(ref elem) => Some(elem),
            _ => None
        }
    }
}

#[derive(Clone)]
struct Element {
    namespace: Option<ElementNamespace>,
    prefix: Option<String>,
    local_name: HtmlString,
    tag_name: HtmlString,
}

impl Element {
    fn is_mathml_text_integration_point(&self) -> bool {
        matches!(self.namespace, Some(ElementNamespace::MathML))
            && (matches!(self.local_name.as_slice(), b"mi" | b"mo" | b"mn" | b"ms" | b"mtext"))
    }

    fn is_html_integration_point(&self) -> bool {
        todo!()
    }
}

pub struct TreeConstructionDispatcher<R: Reader> {
    tokenizer: Tokenizer<R>,
    stack_of_open_elements: Vec<Element>,
    context_element: Option<Node>,
    current_node: Option<Node>,
    insertion_mode: InsertionMode,
    document: Document,
}

impl<R: Reader> TreeConstructionDispatcher<R> {
    pub fn new(tokenizer: Tokenizer<R>) -> Self {
        TreeConstructionDispatcher {
            tokenizer,
            stack_of_open_elements: Vec::new(),
            context_element: None,
            current_node: None,
            insertion_mode: InsertionMode::Initial,
            document: Document::default(),
        }
    }
    fn adjusted_current_node(&self) -> Option<&Node> {
        self.context_element.as_ref().or(self.current_node.as_ref())
    }

    pub fn run(mut self) -> Result<(), R::Error> {
        while let Some(token) = self.tokenizer.next() {
            self.process_token(token?);
        }

        // eof token
        self.process_token_via_insertion_mode(None);
        Ok(())
    }

    fn process_token(&mut self, token: Token) {
        let adjusted_current_elem = self.adjusted_current_node().and_then(|node| node.as_element());
        if self.stack_of_open_elements.is_empty()
            || matches!(adjusted_current_elem.and_then(|elem| elem.namespace.as_ref()), Some(ElementNamespace::HTML))
            || (adjusted_current_elem.map_or(false, |elem| elem.is_mathml_text_integration_point())
                && (matches!(token, Token::StartTag(ref tag) if !matches!(&tag.name[..], b"mglyph" | b"malignmark"))
                    || matches!(token, Token::String(_))))
            || (matches!(adjusted_current_elem.and_then(|elem| elem.namespace.as_ref()), Some(ElementNamespace::MathML))
                && adjusted_current_elem.map_or(false, |elem| *elem.local_name == b"annotation-xml")
                && matches!(token, Token::StartTag(ref tag) if *tag.name == b"svg"))
            || (adjusted_current_elem.map_or(false, |elem| elem.is_html_integration_point())
                && matches!(token, Token::StartTag(_) | Token::String(_)))
        {
            self.process_token_via_insertion_mode(Some(token))
        } else {
            self.process_token_via_foreign_content(token)
        }
    }

    fn process_token_via_insertion_mode(&mut self, mut token: Option<Token>) {
        match self.insertion_mode {
            InsertionMode::Initial => {
                skip_over_chars!(token, b'\t' | b'\x0A' | b'\x0C' | b' ');
                match token {
                    Some(Token::Comment(s)) => {
                        self.insert_a_comment(s, InsertPosition::DocumentLastChild);
                    }
                    Some(Token::Doctype(doctype)) => {
                        if *doctype.name != b"html" || doctype.public_identifier.is_some() || (doctype.system_identifier.as_ref().map_or(false, |x| **x != b"about:legacy-compat".as_slice())) {
                            self.parse_error();
                        }

                        let public_str = doctype.public_identifier.as_ref().map_or(b"".as_slice(), |x| x.as_slice());
                        let system_str = doctype.system_identifier.as_ref().map_or(b"".as_slice(), |x| x.as_slice());

                        if self.document.srcdoc.is_none() && self.document.parser_cannot_change_the_mode && (
                            doctype.force_quirks
                            // TODO case insensitive comparisons
                            || *doctype.name != b"html"
                            || public_str == b"-//W3O//DTD W3 HTML Strict 3.0//EN//"
                            || public_str== b"-/W3C/DTD HTML 4.0 Transitional/EN" 
                            || public_str== b"HTML"
                            || system_str == b"http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd"
                            || public_str.starts_with(b"+//Silmaril//dtd html Pro v0r11 19970101//")
                            || public_str.starts_with(b"-//AS//DTD HTML 3.0 asWedit + extensions//")
                            || public_str.starts_with(b"-//AdvaSoft Ltd//DTD HTML 3.0 asWedit + extensions//")
                            || public_str.starts_with(b"-//IETF//DTD HTML 2.0 Level 1//")
                            || public_str.starts_with(b"-//IETF//DTD HTML 2.0 Level 2//")
                            || public_str.starts_with(b"-//IETF//DTD HTML 2.0 Strict Level 1//")
                            || public_str.starts_with(b"-//IETF//DTD HTML 2.0 Strict Level 2//")
                            || public_str.starts_with(b"-//IETF//DTD HTML 2.0 Strict//")
                            || public_str.starts_with(b"-//IETF//DTD HTML 2.0//")
                            || public_str.starts_with(b"-//IETF//DTD HTML 2.1E//")
                            || public_str.starts_with(b"-//IETF//DTD HTML 3.0//")
                            || public_str.starts_with(b"-//IETF//DTD HTML 3.2 Final//")
                            || public_str.starts_with(b"-//IETF//DTD HTML 3.2//")
                            || public_str.starts_with(b"-//IETF//DTD HTML 3//")
                            || public_str.starts_with(b"-//IETF//DTD HTML Level 0//")
                            || public_str.starts_with(b"-//IETF//DTD HTML Level 1//")
                            || public_str.starts_with(b"-//IETF//DTD HTML Level 2//")
                            || public_str.starts_with(b"-//IETF//DTD HTML Level 3//")
                            || public_str.starts_with(b"-//IETF//DTD HTML Strict Level 0//")
                            || public_str.starts_with(b"-//IETF//DTD HTML Strict Level 1//")
                            || public_str.starts_with(b"-//IETF//DTD HTML Strict Level 2//")
                            || public_str.starts_with(b"-//IETF//DTD HTML Strict Level 3//")
                            || public_str.starts_with(b"-//IETF//DTD HTML Strict//")
                            || public_str.starts_with(b"-//IETF//DTD HTML//")
                            || public_str.starts_with(b"-//Metrius//DTD Metrius Presentational//")
                            || public_str.starts_with(b"-//Microsoft//DTD Internet Explorer 2.0 HTML Strict//")
                            || public_str.starts_with(b"-//Microsoft//DTD Internet Explorer 2.0 HTML//")
                            || public_str.starts_with(b"-//Microsoft//DTD Internet Explorer 2.0 Tables//")
                            || public_str.starts_with(b"-//Microsoft//DTD Internet Explorer 3.0 HTML Strict//")
                            || public_str.starts_with(b"-//Microsoft//DTD Internet Explorer 3.0 HTML//")
                            || public_str.starts_with(b"-//Microsoft//DTD Internet Explorer 3.0 Tables//")
                            || public_str.starts_with(b"-//Netscape Comm. Corp.//DTD HTML//")
                            || public_str.starts_with(b"-//Netscape Comm. Corp.//DTD Strict HTML//")
                            || public_str.starts_with(b"-//O'Reilly and Associates//DTD HTML 2.0//")
                            || public_str.starts_with(b"-//O'Reilly and Associates//DTD HTML Extended 1.0//")
                            || public_str.starts_with(b"-//O'Reilly and Associates//DTD HTML Extended Relaxed 1.0//")
                            || public_str.starts_with(b"-//SQ//DTD HTML 2.0 HoTMetaL + extensions//")
                            || public_str.starts_with(b"-//SoftQuad Software//DTD HoTMetaL PRO 6.0::19990601::extensions to HTML 4.0//")
                            || public_str.starts_with(b"-//SoftQuad//DTD HoTMetaL PRO 4.0::19971010::extensions to HTML 4.0//")
                            || public_str.starts_with(b"-//Spyglass//DTD HTML 2.0 Extended//")
                            || public_str.starts_with(b"-//Sun Microsystems Corp.//DTD HotJava HTML//")
                            || public_str.starts_with(b"-//Sun Microsystems Corp.//DTD HotJava Strict HTML//")
                            || public_str.starts_with(b"-//W3C//DTD HTML 3 1995-03-24//")
                            || public_str.starts_with(b"-//W3C//DTD HTML 3.2 Draft//")
                            || public_str.starts_with(b"-//W3C//DTD HTML 3.2 Final//")
                            || public_str.starts_with(b"-//W3C//DTD HTML 3.2//")
                            || public_str.starts_with(b"-//W3C//DTD HTML 3.2S Draft//")
                            || public_str.starts_with(b"-//W3C//DTD HTML 4.0 Frameset//")
                            || public_str.starts_with(b"-//W3C//DTD HTML 4.0 Transitional//")
                            || public_str.starts_with(b"-//W3C//DTD HTML Experimental 19960712//")
                            || public_str.starts_with(b"-//W3C//DTD HTML Experimental 970421//")
                            || public_str.starts_with(b"-//W3C//DTD W3 HTML//")
                            || public_str.starts_with(b"-//W3O//DTD W3 HTML 3.0//")
                            || public_str.starts_with(b"-//WebTechs//DTD Mozilla HTML 2.0//")
                            || public_str.starts_with(b"-//WebTechs//DTD Mozilla HTML//")
                            || (doctype.system_identifier.is_none() && public_str.starts_with(b"-//W3C//DTD HTML 4.01 Frameset//"))
                            || (doctype.system_identifier.is_none() && public_str.starts_with(b"-//W3C//DTD HTML 4.01 Transitional//" ))
                        ) {
                            self.document.quirks_mode = true;
                        } else if self.document.srcdoc.is_none() && !self.document.parser_cannot_change_the_mode && (
                            // TODO case insensitive comparisons
                            public_str.starts_with(b"-//W3C//DTD XHTML 1.0 Frameset//")
                            || public_str.starts_with(b"-//W3C//DTD XHTML 1.0 Transitional//")
                            || (doctype.system_identifier.is_some() && public_str.starts_with(b"-//W3C//DTD HTML 4.01 Frameset//"))
                            || (doctype.system_identifier.is_some() && public_str.starts_with(b"-//W3C//DTD HTML 4.01 Transitional//" ))
                        ) {
                            self.document.limited_quirks_mode = true;
                        }

                        let node = Node::doctype(Doctype {
                            name: doctype.name,
                            public_identifier: doctype.public_identifier,
                            system_identifier: doctype.system_identifier,
                        });
                        self.document.nodes.push(node);

                        self.insertion_mode = InsertionMode::BeforeHtml;
                    }
                    token => {
                        if self.document.srcdoc.is_none() {
                            self.parse_error();
                        }

                        if self.document.parser_cannot_change_the_mode {
                            self.document.quirks_mode = true;
                        }

                        self.insertion_mode = InsertionMode::BeforeHtml;
                        self.process_token_via_insertion_mode(token);
                    }
                }
            }
            InsertionMode::BeforeHtml => {
                skip_over_chars!(token, b'\t' | b'\x0A' | b'\x0C' | b' ');
                match token {
                    Some(Token::Doctype(_)) => {
                        // ignore the token
                    }
                    Some(Token::Comment(s)) => {
                        self.insert_a_comment(s, InsertPosition::DocumentLastChild);
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"html" => {
                        let element = self.create_an_element_for_the_token(token.unwrap(), ElementNamespace::HTML, Some(&Node::document(self.document.clone())));
                        self.document.nodes.push(Node::element(element.clone()));
                        self.stack_of_open_elements.push(element);
                        self.insertion_mode = InsertionMode::BeforeHead;
                    }
                    Some(Token::EndTag(ref tag)) if *tag.name != b"head" && *tag.name != b"body" && *tag.name != b"html" && *tag.name != b"br" => {
                        self.parse_error();
                    }
                    _ => {
                        let element = Element {
                            namespace: Some(ElementNamespace::HTML),
                            prefix: None,
                            local_name: b"html".as_slice().to_owned().into(),
                            tag_name: b"html".as_slice().to_owned().into(),
                        };

                        let node = Node::element(element);
                        self.insertion_mode = InsertionMode::BeforeHead;
                    }
                }
            }
            _ => todo!()
        }
    }

    fn process_token_via_foreign_content(&mut self, _token: Token) {
        todo!()
    }

    fn insert_a_comment(&mut self, _comment_string: HtmlString, _position: InsertPosition) {
        todo!()
    }
    
    fn parse_error(&mut self) {
        todo!()
    }

    fn create_an_element_for_the_token(&mut self, _token: Token, _namespace: ElementNamespace, _intended_parent: Option<&Node>) -> Element {
        todo!()
    }
}
