use crate::{Reader, Token, Tokenizer, HtmlString, StartTag, State};

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

#[derive(Clone, Copy)]
enum InsertionMode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InBody,
    InHead,
    InHeadNoscript,
    Text,
    AfterHead,
    InTemplate,
    InFrameset
        
}

macro_rules! skip_over_chars {
    ($token:expr, $($chars:pat)|*) => {
        handle_string_prefix!($token, $($chars)|*, |_| ());
    }
}

macro_rules! handle_string_prefix {
    ($token:expr, $($chars:pat)|*, $callback:expr) => {
        if let Some(Token::String(ref mut string)) = $token {
            let index = string.iter().enumerate().find(|(_, x)| !matches!(x, $($chars)|*)).map(|(i, _)| i).unwrap_or(string.len());
            let substring: &[u8] = &string[..index];
            $callback(substring);
            string.copy_within(index.., 0);
            string.truncate(index);
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

#[derive(Clone, Default)]
struct Element {
    namespace: Option<ElementNamespace>,
    prefix: Option<String>,
    local_name: HtmlString,
    tag_name: HtmlString,
    // TODO: script-only
    parser_document: Option<Document>,
    force_async: bool,
    already_started: bool,
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

enum ElementOrMarker {
    Element(Node),
    Marker,
}

pub struct TreeConstructionDispatcher<R: Reader> {
    tokenizer: Tokenizer<R>,
    stack_of_open_elements: Vec<Node>,
    context_element: Option<Node>,
    head_element_pointer: Option<Node>,
    insertion_mode: InsertionMode,
    original_insertion_mode: Option<InsertionMode>,
    document: Document,
    scripting: bool,
    fragment_parsing: bool,
    // "if the parser was invoked via document.write() or document.writeln() methods"
    invoked_via_document_write: bool,
    list_of_active_formatting_elements: Vec<ElementOrMarker>,
    frameset_ok: bool,
    stack_of_template_insertion_modes: Vec<InsertionMode>,
}

impl<R: Reader> TreeConstructionDispatcher<R> {
    pub fn new(tokenizer: Tokenizer<R>) -> Self {
        TreeConstructionDispatcher {
            tokenizer,
            stack_of_open_elements: Vec::new(),
            context_element: None,
            head_element_pointer: None,
            insertion_mode: InsertionMode::Initial,
            original_insertion_mode: None,
            document: Document::default(),
            scripting: false,
            fragment_parsing: false,
            invoked_via_document_write: false,
            list_of_active_formatting_elements: Vec::new(),
            frameset_ok: true,
            stack_of_template_insertion_modes: Vec::new(),
        }
    }

    fn current_node(&self) -> Option<&Node> {
        self.stack_of_open_elements.last()
    }

    fn adjusted_current_node(&self) -> Option<&Node> {
        self.context_element.as_ref().or_else(|| self.current_node())
    }

    fn current_template_insertion_mode(&self) -> Option<&InsertionMode> {
        self.stack_of_template_insertion_modes.last()
    }

    pub fn run(mut self) -> Result<(), R::Error> {
        while let Some(token) = self.tokenizer.next() {
            self.process_token(token?);
        }

        // eof token
        self.process_token_via_insertion_mode(self.insertion_mode, None);
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
            self.process_token_via_insertion_mode(self.insertion_mode, Some(token))
        } else {
            self.process_token_via_foreign_content(token)
        }
    }

    fn process_token_via_insertion_mode(&mut self, insertion_mode: InsertionMode, mut token: Option<Token>) {
        match insertion_mode {
            InsertionMode::Initial => {
                skip_over_chars!(token, b'\t' | b'\x0A' | b'\x0C' | b' ');
                match token {
                    Some(Token::Comment(s)) => {
                        self.insert_a_comment(s, Some(InsertPosition::DocumentLastChild));
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
                        self.process_token_via_insertion_mode(self.insertion_mode, token);
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
                        self.insert_a_comment(s, Some(InsertPosition::DocumentLastChild));
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"html" => {
                        let element = self.create_an_element_for_the_token(token.unwrap(), ElementNamespace::HTML, Some(&Node::document(self.document.clone())));
                        let node = Node::element(element);
                        self.document.nodes.push(node.clone());
                        self.stack_of_open_elements.push(node);
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
                            ..Element::default()
                        };

                        let mut node = Node::element(element);
                        node.node_document = Some(self.document.clone());
                        self.document.nodes.push(node.clone());
                        self.stack_of_open_elements.push(node);
                        self.insertion_mode = InsertionMode::BeforeHead;
                        self.process_token_via_insertion_mode(self.insertion_mode, token);
                    }
                }
            }
            InsertionMode::BeforeHead => {
                skip_over_chars!(token, b'\t' | b'\x0A' | b'\x0C' | b' ');
                match token {
                    Some(Token::Comment(s)) => {
                        self.insert_a_comment(s, None);
                    }
                    Some(Token::Doctype(doctype)) => {
                        self.parse_error();
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"html" => {
                        self.process_token_via_insertion_mode(InsertionMode::InBody, token);
                    }
                    Some(Token::EndTag(ref tag)) if *tag.name != b"head" && *tag.name != b"body" && *tag.name != b"html" && *tag.name != b"br" => {
                        self.parse_error();
                    }
                    token => {
                        let node = self.insert_an_element_for_a_token(Token::StartTag(StartTag {
                            name: b"head".as_slice().to_owned().into(),
                            ..StartTag::default()
                        }));
                        self.head_element_pointer = Some(node.clone());
                        self.insertion_mode = InsertionMode::InHead;
                        self.process_token_via_insertion_mode(self.insertion_mode, token);
                    }
                }
            }
            InsertionMode::InHead => {
                handle_string_prefix!(token, b'\t' | b'\x0A' | b'\x0C' | b' ', |substring| {
                    self.insert_a_character(substring);
                });
                match token {
                    Some(Token::Comment(s)) => {
                        self.insert_a_comment(s, None);
                    }
                    Some(Token::Doctype(doctype)) => {
                        self.parse_error();
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"html" => {
                        self.process_token_via_insertion_mode(InsertionMode::InBody, token);
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"base" | b"basefont" | b"bgsound" | b"link") => {
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.stack_of_open_elements.pop().expect("no current node");
                        // TODO: acknowledge self-closing flag
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"meta" => {
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.stack_of_open_elements.pop().expect("no current node");
                        // TODO: acknowledge self-closing flag
                        // TODO: speculative HTML parsing related to meta charset
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"title" => {
                        self.generic_rcdata_element_parsing_algorithm(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"noframes" | b"style") => {
                        self.generic_rawtext_element_parsing_algorithm(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"noscript" => {
                        if self.scripting {
                            self.generic_rawtext_element_parsing_algorithm(token.unwrap());
                        } else {
                            self.insert_an_element_for_a_token(token.unwrap());
                            self.insertion_mode = InsertionMode::InHeadNoscript;
                        }
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"script" => {
                        let adjusted_insert_location = self.appropriate_place_for_inserting_a_node();
                        let mut elem = self.create_an_element_for_the_token(token.unwrap(), ElementNamespace::HTML, None);
                        elem.parser_document = Some(self.document.clone());
                        elem.force_async = false;
                        if self.fragment_parsing {
                            elem.already_started = true;
                        }
                        if self.invoked_via_document_write {
                            elem.already_started = true;
                        }
                        let node = Node::element(elem);
                        self.insert_element(node.clone(), adjusted_insert_location);
                        self.stack_of_open_elements.push(node);
                        // TODO: use emitter API
                        self.tokenizer.set_state(State::ScriptData);
                        self.original_insertion_mode = Some(self.insertion_mode);
                        self.insertion_mode = InsertionMode::Text;
                    }
                    Some(Token::EndTag(ref tag)) if *tag.name == b"head" => {
                        let head_element = self.stack_of_open_elements.pop().unwrap();
                        debug_assert_eq!(*head_element.as_element().unwrap().tag_name, b"head");
                        debug_assert_eq!(*head_element.as_element().unwrap().local_name, b"head");
                        self.insertion_mode = InsertionMode::AfterHead;
                    }
                    Some(Token::EndTag(ref tag)) if !matches!(tag.name.as_slice(), b"body" | b"html" | b"br") => {
                        // any other end tag
                        self.parse_error();
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"template" => {
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.list_of_active_formatting_elements.push(ElementOrMarker::Marker);
                        self.frameset_ok = false;
                        self.insertion_mode = InsertionMode::InTemplate;
                        self.stack_of_template_insertion_modes.push(InsertionMode::InTemplate);
                    }
                    Some(Token::EndTag(ref tag)) if *tag.name == b"template" => {
                        if self.stack_of_open_elements.iter().filter_map(|x| x.as_element()).filter(|elem| *elem.tag_name == b"template").next().is_none() {
                            self.parse_error();
                            return;
                        }

                        self.generate_all_implied_end_tags_thoroughly();

                        let mut emitted_parse_error = false;

                        let template_elem = loop {
                            match self.stack_of_open_elements.pop() {
                                Some(node) => {
                                    if node.as_element().map_or(false, |x| *x.tag_name == b"template") {
                                        break node;
                                    } else if !emitted_parse_error {
                                        self.parse_error();
                                        emitted_parse_error = true;
                                    }
                                }
                                None => {
                                    unreachable!("checked stack of open elements before");
                                }
                            }
                        };

                        self.clear_list_of_active_formatting_elements_up_to_the_last_marker();
                        self.stack_of_template_insertion_modes.pop().expect("no template insertion mode?");
                        self.reset_the_insertion_mode_appropriately();
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"head" => {
                        self.parse_error();
                    }
                    token => {
                        let head_element = self.stack_of_open_elements.pop().expect("expected head element");
                        debug_assert_eq!(*head_element.as_element().unwrap().tag_name, b"head");
                        debug_assert_eq!(*head_element.as_element().unwrap().local_name, b"head");
                        self.insertion_mode = InsertionMode::AfterHead;
                        self.process_token_via_insertion_mode(self.insertion_mode, token);
                    }
                }
            }
            InsertionMode::InHeadNoscript => {
                handle_string_prefix!(token, b'\t' | b'\x0A' | b'\x0C' | b' ', |substring: &[u8]| {
                    let new_token = Some(Token::String(substring.to_owned().into()));
                    self.process_token_via_insertion_mode(InsertionMode::InHead, new_token);
                });

                match token {
                    Some(Token::Doctype(_)) => {
                        self.parse_error();
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"html" => {
                        self.process_token_via_insertion_mode(InsertionMode::InBody, token);
                    }
                    Some(Token::EndTag(ref tag)) if *tag.name == b"noscript" => {
                        let node = self.stack_of_open_elements.pop().expect("no current node?");
                        debug_assert_eq!(*node.as_element().unwrap().tag_name, b"noscript");
                        debug_assert_eq!(*node.as_element().unwrap().local_name, b"noscript");
                        debug_assert_eq!(*self.current_node().unwrap().as_element().unwrap().tag_name, b"head");
                        self.insertion_mode = InsertionMode::InHead;
                    }
                    Some(Token::Comment(_)) => {
                        self.process_token_via_insertion_mode(InsertionMode::InHead, token);
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"basefont" | b"bgsound" | b"link" | b"meta" | b"noframes" | b"style") => {
                        self.process_token_via_insertion_mode(InsertionMode::InHead, token);
                    }
                    Some(Token::EndTag(ref tag)) if *tag.name != b"br" => {
                        self.parse_error();
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"head" | b"noscript") => {
                        self.parse_error();
                    }
                    token => {
                        self.parse_error();
                        let node = self.stack_of_open_elements.pop().expect("no current node");
                        debug_assert_eq!(*self.current_node().unwrap().as_element().unwrap().tag_name, b"head");
                        self.insertion_mode = InsertionMode::InHead;
                        self.process_token_via_insertion_mode(self.insertion_mode, token);
                    }
                }
            }
            InsertionMode::AfterHead => {
                handle_string_prefix!(token, b'\t' | b'\x0A' | b'\x0C' | b' ', |substring| {
                    self.insert_a_character(substring);
                });

                match token {
                    Some(Token::Comment(s)) => {
                        self.insert_a_comment(s, None);
                    }
                    Some(Token::Doctype(_)) => {
                        self.parse_error();
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"html" => {
                        self.process_token_via_insertion_mode(InsertionMode::InBody, token);
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"body" => {
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.frameset_ok = false;
                        self.insertion_mode = InsertionMode::InBody;
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"frameset" => {
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.insertion_mode = InsertionMode::InFrameset;
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"base" | b"basefont" | b"bgsound" | b"link" | b"meta" | b"noframes" | b"script" | b"style" | b"template" | b"title") => {
                        self.parse_error();
                        let node = self.head_element_pointer.clone().unwrap();
                        let i = self.stack_of_open_elements.len();
                        self.stack_of_open_elements.push(node);
                        self.process_token_via_insertion_mode(InsertionMode::InHead, token);
                        // XXX: unclear if this is correct
                        self.stack_of_open_elements.remove(i);
                    }
                    Some(Token::EndTag(ref tag)) if *tag.name == b"template" => {
                        self.process_token_via_insertion_mode(InsertionMode::InHead, token);
                    }
                    Some(Token::EndTag(ref tag)) if !matches!(tag.name.as_slice(), b"body" | b"html" | b"br") => {
                        self.parse_error();
                    }
                    Some(Token::StartTag(ref tag)) if *tag.name == b"head" => {
                        self.parse_error();
                    }
                    token => {
                        self.insert_an_element_for_a_token(Token::StartTag(StartTag {
                            name: b"body".as_slice().to_owned().into(),
                            ..StartTag::default()
                        }));
                        self.insertion_mode = InsertionMode::InBody;
                        self.process_token_via_insertion_mode(self.insertion_mode, token);
                    }
                }
            }
            _ => todo!()
        }
    }

    fn process_token_via_foreign_content(&mut self, _token: Token) {
        todo!()
    }

    fn insert_a_comment(&mut self, _comment_string: HtmlString, _position: Option<InsertPosition>) {
        todo!()
    }
    
    fn parse_error(&mut self) {
        todo!()
    }

    fn insert_a_character(&mut self, characters: &[u8]) {
        todo!()
    }

    fn create_an_element_for_the_token(&mut self, _token: Token, _namespace: ElementNamespace, _intended_parent: Option<&Node>) -> Element {
        todo!()
    }

    fn insert_an_element_for_a_token(&mut self, token: Token) -> Node {
        todo!()
    }

    fn generic_rcdata_element_parsing_algorithm(&mut self, _token: Token) {
        todo!()
    }

    fn generic_rawtext_element_parsing_algorithm(&mut self, _token: Token) {
        todo!()
    }

    fn appropriate_place_for_inserting_a_node(&mut self) -> InsertPosition {
        todo!()
    }

    fn insert_element(&mut self, node: Node, position: InsertPosition) {
        todo!()
    }

    fn generate_all_implied_end_tags_thoroughly(&mut self) {
        todo!()
    }

    fn clear_list_of_active_formatting_elements_up_to_the_last_marker(&mut self) {
        todo!()
    }

    fn reset_the_insertion_mode_appropriately(&mut self) {
        todo!()
    }
}
