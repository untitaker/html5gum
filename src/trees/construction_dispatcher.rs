#![allow(unused)]

use std::collections::BTreeMap;

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
    InFrameset,
    AfterBody,
    InTable,
    InSelect,
    InSelectInTable,
    InRow,
    InTableBody,
    InCaption,
    InCell,
    InTableText,
    InColumnGroup,
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
            if string.is_empty() {
                return
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
    fn as_element_mut(&mut self) -> Option<&mut Element> {
        match self.inner {
            NodeInner::Element(ref mut elem) => Some(elem),
            _ => None
        }
    }

    fn is_element(&self, tag_name: &[u8]) -> bool {
        self.as_element().map_or(false, |elem| *elem.tag_name == tag_name)
    }

    fn is_special(&self) -> bool {
        todo!()
    }

    fn same_identity(&self, other: &Node) -> bool {
        todo!()
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
    attributes: BTreeMap<HtmlString, HtmlString>
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

impl ElementOrMarker {
    fn as_element(&self) -> Option<&Node> {
        match self {
            ElementOrMarker::Element(elem) => Some(elem),
            ElementOrMarker::Marker => None
        }
    }
}

pub struct TreeConstructionDispatcher<R: Reader> {
    tokenizer: Tokenizer<R>,
    stack_of_open_elements: Vec<Node>,
    context_element: Option<Node>,
    head_element_pointer: Option<Node>,
    form_element_pointer: Option<Node>,
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
    pending_table_character_tokens: Vec<u8>,
    foster_parenting: bool,
}

impl<R: Reader> TreeConstructionDispatcher<R> {
    pub fn new(tokenizer: Tokenizer<R>) -> Self {
        TreeConstructionDispatcher {
            tokenizer,
            stack_of_open_elements: Vec::new(),
            context_element: None,
            head_element_pointer: None,
            form_element_pointer: None,
            insertion_mode: InsertionMode::Initial,
            original_insertion_mode: None,
            document: Document::default(),
            scripting: false,
            fragment_parsing: false,
            invoked_via_document_write: false,
            list_of_active_formatting_elements: Vec::new(),
            frameset_ok: true,
            stack_of_template_insertion_modes: Vec::new(),
            pending_table_character_tokens: Vec::new(),
            foster_parenting: false,
        }
    }

    fn current_node(&self) -> Option<&Node> {
        self.stack_of_open_elements.last()
    }

    fn current_node_mut(&mut self) -> Option<&mut Node> {
        self.stack_of_open_elements.last_mut()
    }

    fn adjusted_current_node(&self) -> Option<&Node> {
        self.context_element.as_ref().or_else(|| self.current_node())
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
                        self.reprocess_token(token);
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
                        self.reprocess_token(token);
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
                        self.reprocess_token(token);
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
                        self.reprocess_token(token);
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
                        self.reprocess_token(token);
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
                        self.reprocess_token(token);
                    }
                }
            }
            InsertionMode::InBody => {
                // TODO: this is bogus, doesn't handle "\t\0somethingelse" correctly
                handle_string_prefix!(token, b'\0', |substring: &[u8]| {
                    self.parse_error();
                });

                handle_string_prefix!(token, b'\t' | b'\x0A' | b'\x0C' | b' ', |substring: &[u8]| {
                    self.reconstruct_the_active_formatting_elements();
                    self.insert_a_character(&substring);
                });

                match token {
                    Some(Token::String(s)) => {
                        self.reconstruct_the_active_formatting_elements();
                        self.insert_a_character(&s);
                        self.frameset_ok = false;
                    }
                    Some(Token::Comment(s)) => {
                        self.insert_a_comment(s, None);
                    }
                    Some(Token::Doctype(_)) => {
                        self.parse_error();
                    }
                    Some(Token::StartTag(tag)) if *tag.name == b"html" => {
                        self.parse_error();
                        // TODO: node needs to become shared on clone
                        let has_template_elem = self.stack_of_open_elements.iter().any(|node| node.as_element().map_or(false, |elem| *elem.tag_name == b"template"));
                        if !has_template_elem {
                            if let Some(node) = self.stack_of_open_elements.first_mut() {
                                if let Some(elem) = node.as_element_mut() {
                                    for (key, value) in tag.attributes {
                                        elem.attributes.entry(key).or_insert(value);
                                    }
                                }
                            }
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"base" | b"basefont" | b"bgsound" | b"link" | b"meta" | b"noframes" | b"script" | b"style" | b"template" | b"title") => {
                        self.process_token_via_insertion_mode(InsertionMode::InHead, token);
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"template") => {
                        self.process_token_via_insertion_mode(InsertionMode::InHead, token);
                    }
                    Some(Token::StartTag(tag)) if matches!(tag.name.as_slice(), b"body") => {
                        self.parse_error();

                        let has_template_elem = self.stack_of_open_elements.iter().any(|node| node.as_element().map_or(false, |elem| *elem.tag_name == b"template"));

                        if !has_template_elem {
                            if let Some(node) = self.stack_of_open_elements.get_mut(1) {
                                if let Some(elem) = node.as_element_mut() {
                                    if *elem.tag_name == b"body" {
                                        self.frameset_ok = false;
                                        for (key, value) in tag.attributes {
                                            elem.attributes.entry(key).or_insert(value);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"frameset") => {
                        self.parse_error();
                        let dont_ignore_token = self.frameset_ok && self.stack_of_open_elements.get(1).and_then(|node| node.as_element()).map(|elem| *elem.tag_name == b"body").unwrap_or(false);

                        if dont_ignore_token {
                            self.stack_of_open_elements.truncate(2);
                            self.stack_of_open_elements.pop().expect("no body element?");
                            self.insert_an_element_for_a_token(token.unwrap());
                            self.insertion_mode = InsertionMode::InFrameset;
                        }
                    }
                    None => {
                        if !self.stack_of_open_elements.is_empty() {
                            self.process_token_via_insertion_mode(InsertionMode::InTemplate, token);
                        } else {
                            for node in &self.stack_of_open_elements {
                                if let Some(elem) = node.as_element() {
                                    if !matches!(elem.tag_name.as_slice(), b"dd" | b"dt" | b"li" | b"optgroup" | b"option" | b"p" | b"rb" | b"rp" | b"rt" | b"rtc" | b"tbody" | b"td" | b"tfoot" | b"th" | b"thead" | b"tr" | b"body" | b"html") {
                                        self.parse_error();
                                        break;
                                    }
                                }
                            }
                            self.stop_parsing();
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"body" | b"html") => {
                        if !self.has_element_in_scope(b"body") {
                            self.parse_error();
                        } else {
                            for node in &self.stack_of_open_elements {
                                if let Some(elem) = node.as_element() {
                                    if !matches!(elem.tag_name.as_slice(), b"dd" | b"dt" | b"li" | b"optgroup" | b"option" | b"p" | b"rb" | b"rp" | b"rt" | b"rtc" | b"tbody" | b"td" | b"tfoot" | b"th" | b"thead" | b"tr" | b"body" | b"html") {
                                        self.parse_error();
                                        break;
                                    }
                                }
                            }

                            self.insertion_mode = InsertionMode::AfterBody;

                            if tag.name.as_slice() == b"html" {
                                self.reprocess_token(token);
                            }
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"address" | b"article" | b"aside" | b"blockquote" | b"center" | b"details" | b"dialog" | b"dir" | b"div" | b"dl" | b"fieldset" | b"figcaption" | b"figure" | b"footer" | b"header" | b"hgroup" | b"main" | b"menu" | b"nav" | b"ol" | b"p" | b"section" | b"summary" | b"ul") => {
                        if self.has_element_in_button_scope(b"p") {
                            self.close_a_p_element();
                        }

                        self.insert_an_element_for_a_token(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6") => {
                        if self.has_element_in_button_scope(b"p") {
                            self.close_a_p_element();
                        }

                        if self.current_node().and_then(|node| node.as_element()).map_or(false, |elem| matches!(elem.tag_name.as_slice(), b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6")) {
                            self.parse_error();
                            self.stack_of_open_elements.pop().unwrap();
                        }

                        self.insert_an_element_for_a_token(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"pre" | b"listing") => {
                        if self.has_element_in_button_scope(b"p") {
                            self.close_a_p_element();
                        }

                        self.insert_an_element_for_a_token(token.unwrap());

                        if let Some(Token::String(ref mut string)) = self.peek_token() {
                            if string.starts_with(b"\n") {
                                let len = string.len();
                                string.copy_within(1.., 0);
                                string.truncate(len - 1);
                            }
                        }

                        self.frameset_ok = false;
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"form") => {
                        let has_template_elem = self.stack_of_open_elements.iter().any(|node| node.as_element().map_or(false, |elem| *elem.tag_name == b"template"));
                        if self.form_element_pointer.is_none() && !has_template_elem {
                            self.parse_error();
                        } else {
                            if self.has_element_in_button_scope(b"p") {
                                self.close_a_p_element();
                            }

                            let node = self.insert_an_element_for_a_token(token.unwrap());
                            if !has_template_elem {
                                self.form_element_pointer = Some(node);
                            }
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"li") => {
                        self.frameset_ok = false;
                        let node = self.current_node().unwrap().clone();

                        // Loop:
                        loop {
                            if node.is_element(b"li") {
                                self.generate_implied_end_tags(&[b"li"]);
                                if !self.current_node().map_or(false, |node| node.is_element(b"li")) {
                                    self.parse_error();
                                }

                                while let Some(node) = self.stack_of_open_elements.pop() {
                                    if node.is_element(b"li") {
                                        break;
                                    }
                                }

                                // "jump to the step labeled done below"
                                break;
                            }

                            if node.is_special() && !node.is_element(b"address") && !node.is_element(b"div") && !node.is_element(b"p")  {
                                // "jump to the step labeled done below"
                                break;
                            } else {
                                // "Otherwise, set node to the previous entry in the stack of open elements and return to the step labeled loop."
                                // TODO: what does "previous" mean
                                todo!();
                            }
                        }

                        // Done:
                        if self.has_element_in_button_scope(b"p") {
                            self.close_a_p_element();
                        }

                        self.insert_an_element_for_a_token(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"dd" | b"dt") => {
                        self.frameset_ok = false;
                        let node = self.current_node().unwrap().clone();
                        // Loop:
                        loop {
                            if node.is_element(b"dd") {
                                self.generate_implied_end_tags(&[b"dd"]);
                                if !self.current_node().map_or(false, |node| node.is_element(b"dd")) {
                                    self.parse_error();
                                }

                                while let Some(node) = self.stack_of_open_elements.pop() {
                                    if node.is_element(b"dd") {
                                        break;
                                    }
                                }

                                break;
                            }

                            if node.is_element(b"dt") {
                                self.generate_implied_end_tags(&[b"dt"]);
                                if !self.current_node().map_or(false, |node| node.is_element(b"dt")) {
                                    self.parse_error();
                                }

                                while let Some(node) = self.stack_of_open_elements.pop() {
                                    if node.is_element(b"dt") {
                                        break;
                                    }
                                }

                                break;
                            }

                            if node.is_special() && !node.is_element(b"address") && !node.is_element(b"div") && !node.is_element(b"p")  {
                                break;
                            } else {
                                // "Otherwise, set node to the previous entry in the stack of open elements and return to the step labeled loop."
                                // TODO: what does "previous" mean
                                todo!();
                            }
                        }

                        // Done:
                        if self.has_element_in_button_scope(b"p") {
                            self.close_a_p_element();
                        }

                        self.insert_an_element_for_a_token(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"plaintext") => {
                        if self.has_element_in_button_scope(b"p") {
                            self.close_a_p_element();
                        }

                        self.insert_an_element_for_a_token(token.unwrap());
                        // TODO: use emitter API
                        self.tokenizer.set_state(State::PlainText);
                        // TODO: re-read note in spec, optimization potential?
                        //
                        // 'Once a start tag with the tag name "plaintext" has been seen, that will
                        // be the last token ever seen other than character tokens (and the
                        // end-of-file token), because there is no way to switch out of the
                        // PLAINTEXT state.'
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"button") => {
                        if self.has_element_in_scope(b"button") {
                            self.parse_error();
                            self.generate_implied_end_tags(&[]);
                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if node.is_element(b"button") {
                                    break;
                                }
                            }
                            self.reconstruct_the_active_formatting_elements();
                            self.insert_an_element_for_a_token(token.unwrap());
                            self.frameset_ok = false;
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"address" | b"article" | b"blockquote" | b"button" | b"center" | b"details" | b"dir" | b"div" | b"dl" | b"fieldset" | b"figcaption" | b"figure" | b"footer" | b"header" | b"hgroup" | b"listing" | b"main" | b"menu" | b"nav" | b"ol" | b"pre" | b"section" | b"summary" | b"ul") => {
                        if !self.has_element_in_scope(tag.name.as_slice()) {
                            self.parse_error();
                        } else {
                            self.generate_implied_end_tags(&[]);
                            if !self.current_node().map_or(false, |node| node.is_element(tag.name.as_slice())) {
                                self.parse_error();
                            }
                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if node.is_element(tag.name.as_slice()) {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"form") => {
                        let has_template_elem = self.stack_of_open_elements.iter().any(|node| node.as_element().map_or(false, |elem| *elem.tag_name == b"template"));
                        if !has_template_elem {
                            let mut node = self.form_element_pointer.take();
                            if node.as_ref().map_or(true, |node| !self.has_element_in_scope2(|node2| node.same_identity(node2))) {
                                self.parse_error();
                                return;
                            }
                            self.generate_implied_end_tags(&[]);
                            match (self.current_node(), &node) {
                                (Some(a), Some(b)) if a.same_identity(b) => (),
                                (None, None) => (),
                                _ => {
                                    self.parse_error();
                                }
                            }
                            if let Some(ref node) = node {
                                self.stack_of_open_elements.retain(|node2| {
                                    !node2.same_identity(node)
                                });
                            }
                        } else {
                            if !self.has_element_in_scope(b"form") {
                                self.parse_error();
                                return;
                            }
                            self.generate_implied_end_tags(&[]);
                            if !self.current_node().map_or(false, |node| node.is_element(b"form")) {
                                self.parse_error();
                            }

                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if node.is_element(b"form") {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"p") => {
                        if !self.has_element_in_button_scope(b"p") {
                            self.parse_error();
                            self.insert_an_element_for_a_token(Token::StartTag(StartTag {
                                name: b"p".as_slice().to_owned().into(),
                                ..StartTag::default()
                            }));
                        }

                        self.close_a_p_element();
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"li") => {
                        if !self.has_element_in_list_item_scope(b"li") {
                            self.parse_error();
                        } else {
                            self.generate_implied_end_tags(&[b"li"]);
                            if !self.current_node().map_or(false, |node| node.is_element(b"li")) {
                                self.parse_error();
                            }
                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if node.is_element(b"li") {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"dd" | b"dt") => {
                        if !self.has_element_in_scope(&tag.name) {
                            self.parse_error();
                        } else {
                            self.generate_implied_end_tags(&[&tag.name]);
                            if !self.current_node().map_or(false, |node| node.is_element(&tag.name)) {
                                self.parse_error();
                            }

                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if node.is_element(&tag.name) {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6") => {
                        fn is_heading(node: &Node) -> bool {
                            node.is_element(b"h1") || node.is_element(b"h2") || node.is_element(b"h3") || node.is_element(b"h4") || node.is_element(b"h5") || node.is_element(b"h6")
                        }

                        if !self.has_element_in_scope2(is_heading) {
                            self.parse_error();
                        } else {
                            self.generate_implied_end_tags(&[]);
                            if !self.current_node().map_or(false, |node| node.is_element(&tag.name)) {
                                self.parse_error();
                            }
                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if is_heading(&node) {
                                    break;
                                }
                            }
                        }
                    }
                    // > An end tag whose tag name is "sarcasm": Take a deep breath, then act as
                    // > described in the "any other end tag" entry below.
                    //
                    // Already handled by the fallthrough case. There are no other branches before
                    // that that could "catch" this case.
                    //
                    // Also already took many deep breaths while writing this code.
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"a") => {
                        let mut found_a_element = None;
                        for (i, element_or_marker) in self.list_of_active_formatting_elements.iter().enumerate().rev() {
                            match element_or_marker {
                                ElementOrMarker::Marker => break,
                                ElementOrMarker::Element(elem) => {
                                    if elem.is_element(b"a") {
                                        found_a_element = Some(i);
                                        break;
                                    }
                                }
                            }
                        }

                        if let Some(i) = found_a_element {
                            self.parse_error();
                            // TODO: can i pass a reference to a token here?
                            self.run_adoption_agency_algorithm(token.clone().unwrap());
                            // TODO: wrong assumptions?
                            debug_assert!(self.list_of_active_formatting_elements.remove(i).as_element().unwrap().is_element(b"a"));
                        }

                        self.reconstruct_the_active_formatting_elements();
                        let node = self.insert_an_element_for_a_token(token.unwrap());
                        self.list_of_active_formatting_elements.push(ElementOrMarker::Element(node));
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"b" | b"big" | b"code" | b"em" | b"font" | b"i" | b"s" | b"small" | b"strike" | b"strong" | b"tt" | b"u") => {
                        self.reconstruct_the_active_formatting_elements();
                        let node = self.insert_an_element_for_a_token(token.unwrap());
                        self.list_of_active_formatting_elements.push(ElementOrMarker::Element(node));
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"nobr") => {
                        self.reconstruct_the_active_formatting_elements();
                        if self.has_element_in_scope(b"nobr") {
                            self.parse_error();
                            self.run_adoption_agency_algorithm(token.clone().unwrap());
                            self.reconstruct_the_active_formatting_elements();
                        }
                        let node = self.insert_an_element_for_a_token(token.unwrap());
                        self.list_of_active_formatting_elements.push(ElementOrMarker::Element(node));
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"a" | b"b" | b"big" | b"code" | b"em" | b"font" | b"i" | b"nobr" | b"s" | b"small" | b"strike" | b"strong" | b"tt" | b"u") => {
                        self.run_adoption_agency_algorithm(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"applet" | b"marquee" | b"object") => {
                        self.reconstruct_the_active_formatting_elements();
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.list_of_active_formatting_elements.push(ElementOrMarker::Marker);
                        self.frameset_ok = false;
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"applet" | b"marquee" | b"object") => {
                        if !self.has_element_in_scope(&tag.name) {
                            self.parse_error();
                        } else {
                            self.generate_implied_end_tags(&[]);
                            if !self.current_node().map_or(false, |node| node.is_element(&tag.name)) {
                                self.parse_error();
                            }
                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if node.is_element(&tag.name) {
                                    break;
                                }
                            }
                            self.clear_list_of_active_formatting_elements_up_to_the_last_marker();
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"table") => {
                        if !self.document.quirks_mode && self.has_element_in_button_scope(b"p") {
                            self.close_a_p_element();
                        }
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.frameset_ok = false;
                        self.insertion_mode = InsertionMode::InTable;
                    }
                    Some(Token::EndTag(tag)) if matches!(tag.name.as_slice(), b"br") => {
                        self.parse_error();
                        self.process_token_via_insertion_mode(self.insertion_mode, Some(Token::StartTag(StartTag {
                            name: tag.name,
                            ..StartTag::default()
                        })));
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"area" | b"br" | b"embed" | b"img" | b"keygen" | b"wbr") => {
                        self.reconstruct_the_active_formatting_elements();
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.stack_of_open_elements.pop().unwrap();
                        // TODO: acknowledge self-closing flag
                        self.frameset_ok = false;
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"input") => {
                        self.reconstruct_the_active_formatting_elements();
                        // TODO: assumption: token is not mutated by insert_an_element_for_a_token
                        let type_is_hidden = !tag.attributes.get(b"type".as_slice()).map_or(false, |value| **value == b"hidden".as_slice());
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.stack_of_open_elements.pop().unwrap();
                        // TODO: acknowledge self-closing flag
                        if type_is_hidden {
                            self.frameset_ok = false;
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"param" | b"source" | b"track") => {
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.stack_of_open_elements.pop().unwrap();
                        // TODO: acknowledge self-closing flag
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"hr") => {
                        if self.has_element_in_button_scope(b"p") {
                            self.close_a_p_element();
                        }

                        self.insert_an_element_for_a_token(token.unwrap());
                        self.stack_of_open_elements.pop().unwrap();
                        // TODO: acknowledge self-closing flag
                        self.frameset_ok = false;
                    }
                    Some(Token::StartTag(ref mut tag)) if matches!(tag.name.as_slice(), b"image") => {
                        self.parse_error();
                        // "change the token's tag name to img and reprocess it. (Don't ask)"
                        tag.name = b"img".as_slice().to_owned().into();
                        self.reprocess_token(token);
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"textarea") => {
                        self.insert_an_element_for_a_token(token.unwrap());
                        if let Some(Token::String(ref mut string)) = self.peek_token() {
                            if string.starts_with(b"\n") {
                                let len = string.len();
                                string.copy_within(1.., 0);
                                string.truncate(len - 1);
                            }
                        }
                        self.tokenizer.set_state(State::RcData);
                        self.original_insertion_mode = Some(self.insertion_mode);
                        self.frameset_ok = false;
                        self.insertion_mode = InsertionMode::Text;
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"xmp") => {
                        if self.has_element_in_button_scope(b"p") {
                            self.close_a_p_element();
                        }
                        self.reconstruct_the_active_formatting_elements();
                        self.frameset_ok = false;
                        self.generic_rawtext_element_parsing_algorithm(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"iframe") => {
                        self.frameset_ok = false;
                        self.generic_rawtext_element_parsing_algorithm(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"noembed") => {
                        self.generic_rawtext_element_parsing_algorithm(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if self.scripting && matches!(tag.name.as_slice(), b"noscript") => {
                        self.generic_rawtext_element_parsing_algorithm(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"select") => {
                        self.reconstruct_the_active_formatting_elements();
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.frameset_ok = false;
                        if matches!(self.insertion_mode, InsertionMode::InTable | InsertionMode::InCaption | InsertionMode::InTableBody | InsertionMode::InRow | InsertionMode::InCell) {
                            self.insertion_mode = InsertionMode::InSelectInTable;
                        } else {
                            self.insertion_mode = InsertionMode::InSelect;
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"optgroup" | b"option") => {
                        if self.current_node().map_or(false, |node| node.is_element(b"option")) {
                            self.stack_of_open_elements.pop().unwrap();
                        }

                        self.reconstruct_the_active_formatting_elements();
                        self.insert_an_element_for_a_token(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"rb" | b"rtc") => {
                        if self.has_element_in_scope(b"ruby") {
                            self.generate_implied_end_tags(&[]);
                            // TODO: perhaps this needs to be run un-nested?
                            if !self.current_node().map_or(false, |node| node.is_element(b"ruby")) {
                                self.parse_error();
                            }
                        }

                        self.insert_an_element_for_a_token(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"rp" | b"rt") => {
                        if self.has_element_in_scope(b"ruby") {
                            self.generate_implied_end_tags(&[b"rtc"]);
                            // TODO: perhaps this needs to be run un-nested?
                            if !self.current_node().map_or(false, |node| node.is_element(b"ruby") || node.is_element(b"rtc")) {
                                self.parse_error();
                            }
                        }

                        self.insert_an_element_for_a_token(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"math") => {
                        // TODO: perhaps this attribute gets modified later?
                        let self_closing = tag.self_closing;
                        self.reconstruct_the_active_formatting_elements();
                        let mut token = token.unwrap();
                        self.adjust_mathml_attributes(&mut token);
                        self.adjust_foreign_attributes(&mut token);
                        self.insert_a_foreign_element(token, ElementNamespace::MathML);
                        if self_closing {
                            self.stack_of_open_elements.pop().unwrap();
                            // TODO: acknowledge self-closing flag
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"svg") => {
                        // TODO: perhaps this attribute gets modiifed later?
                        let self_closing = tag.self_closing;
                        self.reconstruct_the_active_formatting_elements();
                        let mut token = token.unwrap();
                        self.adjust_svg_attributes(&mut token);
                        self.adjust_foreign_attributes(&mut token);
                        self.insert_a_foreign_element(token, ElementNamespace::SVG);
                        if self_closing {
                            self.stack_of_open_elements.pop().unwrap();
                            // TODO: acknowledge self-closing flag
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"caption" | b"col" | b"colgroup" | b"frame" | b"head" | b"tbody" | b"td" | b"tfoot" | b"th" | b"thead" | b"tr") => {
                        self.parse_error();
                    }
                    Some(token @ Token::StartTag(_)) => {
                        self.reconstruct_the_active_formatting_elements();
                        self.insert_an_element_for_a_token(token);
                        // TODO: debug assert for "ordinary" element
                    }
                    Some(Token::EndTag(ref tag)) => {
                        let mut node = self.current_node().unwrap().clone();
                        // Loop:
                        loop {
                            if node.is_element(&tag.name) {
                                self.generate_implied_end_tags(&[&tag.name]);
                                if self.current_node().map_or(false, |node2| node2.same_identity(&node)) {
                                    self.parse_error();
                                }
                                while let Some(node2) = self.stack_of_open_elements.pop() {
                                    if node.same_identity(&node2) {
                                        break;
                                    }
                                }
                                break;
                            }

                            // TODO: "set node to the previous entry in the stack of open elements"
                            todo!();
                        }
                    }
                    Some(Token::Error(_)) => todo!(),
                }
            }
            InsertionMode::Text => {
                match token {
                    Some(Token::String(s)) => {
                        debug_assert!(s.iter().all(|&x| x != b'\0'));
                        self.insert_a_character(&s);
                    }
                    None => {
                        self.parse_error();
                        if let Some(current_node) = self.current_node_mut() {
                            if current_node.is_element(b"script") {
                                current_node.as_element_mut().unwrap().already_started = true;
                            }
                        }

                        self.stack_of_open_elements.pop().unwrap();
                        self.insertion_mode = self.original_insertion_mode.unwrap();
                        self.reprocess_token(token);
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"script") => {
                        // TODO: implement this entire state. we don't really support scripting
                        let node = self.stack_of_open_elements.pop().unwrap();
                        debug_assert!(node.is_element(b"script"));
                    }
                    Some(Token::EndTag(ref tag)) => {
                        self.stack_of_open_elements.pop().unwrap();
                        self.insertion_mode = self.original_insertion_mode.unwrap();
                    }
                    _ => {
                        // undefined transitions in spec
                        unreachable!();
                    }
                }
            }
            InsertionMode::InTable => {
                match token {
                    Some(Token::String(_)) if self.current_node().map_or(false, |node| node.is_element(b"table") || node.is_element(b"tbody") || node.is_element(b"tfoot") || node.is_element(b"thead") || node.is_element(b"tr")) => {
                        self.pending_table_character_tokens.clear();
                        self.original_insertion_mode = Some(self.insertion_mode);
                        self.insertion_mode = InsertionMode::InTableText;
                        self.reprocess_token(token);
                    }
                    Some(Token::Comment(s)) => {
                        self.insert_a_comment(s, None);
                    }
                    Some(Token::Doctype(doctype)) => {
                        self.parse_error();
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"caption") => {
                        self.clear_stack_back_to_a_table_context();
                        self.list_of_active_formatting_elements.push(ElementOrMarker::Marker);
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.insertion_mode = InsertionMode::InCaption;
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"colgroup") => {
                        self.clear_stack_back_to_a_table_context();
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.insertion_mode = InsertionMode::InColumnGroup;
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"col") => {
                        self.clear_stack_back_to_a_table_context();
                        self.insert_an_element_for_a_token(Token::StartTag(StartTag {
                            name: b"colgroup".as_slice().to_owned().into(),
                            ..StartTag::default()
                        }));
                        self.insertion_mode = InsertionMode::InColumnGroup;
                        self.reprocess_token(token);
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"tbody" | b"tfoot" | b"thead") => {
                        self.clear_stack_back_to_a_table_context();
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.insertion_mode = InsertionMode::InTableBody;
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"td" | b"th" | b"tr") => {
                        self.clear_stack_back_to_a_table_context();
                        self.insert_an_element_for_a_token(Token::StartTag(StartTag {
                            name: b"tbody".as_slice().to_owned().into(),
                            ..StartTag::default()
                        }));
                        self.insertion_mode = InsertionMode::InTableBody;
                        self.reprocess_token(token);
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"table") => {
                        self.parse_error();
                        if self.has_element_in_table_scope(b"table") {
                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if node.is_element(b"table") {
                                    break;
                                }
                            }

                            self.reset_the_insertion_mode_appropriately();
                            self.reprocess_token(token);
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"table") => {
                        if !self.has_element_in_table_scope(b"table") {
                            self.parse_error();
                        } else {
                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if node.is_element(b"table") {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"body" | b"caption" | b"col" | b"colgroup" | b"html" | b"tbody" | b"td" | b"tfoot" | b"th" | b"thead" | b"tr") => {
                        self.parse_error();
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"style" | b"script" | b"template") => {
                        self.process_token_via_insertion_mode(InsertionMode::InHead, token);
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"template") => {
                        self.process_token_via_insertion_mode(InsertionMode::InHead, token);
                    }
                    // TODO: ascii-case insensitive match for "hidden"
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"input") && tag.attributes.get(b"type".as_slice()).map_or(false, |value| **value == b"hidden") => {
                        self.parse_error();
                        let node = self.insert_an_element_for_a_token(token.unwrap());
                        let node2 = self.stack_of_open_elements.pop().unwrap();
                        debug_assert!(node.same_identity(&node2));
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"form") => {
                        self.parse_error();
                        if self.stack_of_open_elements.iter().any(|node| node.is_element(b"template")) || self.form_element_pointer.is_some() {
                            // ignore the token
                        } else {
                            let node = self.insert_an_element_for_a_token(token.unwrap());
                            let node2 = self.stack_of_open_elements.pop().unwrap();
                            debug_assert!(node.same_identity(&node2));
                            self.form_element_pointer = Some(node);
                        }

                    }
                    None => {
                        self.process_token_via_insertion_mode(InsertionMode::InBody, token);
                    }
                    token => {
                        self.parse_error();
                        self.foster_parenting = true;
                        self.process_token_via_insertion_mode(InsertionMode::InBody, token);
                        self.foster_parenting = false;
                    }
                }
            }
            InsertionMode::InTableText => {
                match token {
                    Some(Token::String(s)) => {
                        for &c in &*s {
                            if c == b'\0' {
                                self.parse_error();
                            } else {
                                self.pending_table_character_tokens.push(c);
                            }
                        }
                    }
                    token => {
                        if self.pending_table_character_tokens.iter().any(|x| !x.is_ascii_whitespace()) {
                            // > [...] then this is a parse error: reprocess the character tokens in the
                            // > pending table character tokens list using the rules given in the
                            // > "anything else" entry in the "in table" insertion mode.
                            //
                            // TODO: two parse errors? the InTable insertion mode also emits a
                            // parse error
                            self.parse_error();
                            self.foster_parenting = true;
                            // XXX: inefficient clone
                            let pending = self.pending_table_character_tokens.clone();
                            // TODO: clear pending characters?
                            self.process_token_via_insertion_mode(InsertionMode::InBody, Some(Token::String(pending.into())));
                            self.foster_parenting = false;
                        } else {
                            // XXX: inefficient clone
                            let pending = self.pending_table_character_tokens.clone();
                            // TODO: clear pending characters?
                            self.insert_a_character(&pending);
                        }

                        self.insertion_mode = self.original_insertion_mode.unwrap();
                        self.reprocess_token(token);
                    }
                }
            }
            InsertionMode::InCaption => {
                match token {
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"caption") => {
                        self.handle_in_caption_inner();
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"caption" | b"col" | b"colgroup" | b"tbody" | b"td" | b"tfoot" | b"th" | b"thead" | b"tr") => {
                        if self.handle_in_caption_inner() {
                            self.reprocess_token(token);
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"table") => {
                        if self.handle_in_caption_inner() {
                            self.reprocess_token(token);
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"body" | b"col" | b"colgroup" | b"html" | b"tbody" | b"td" | b"tfoot" | b"th" | b"thead" | b"tr") => {
                        self.parse_error();
                    }
                    _ => {
                        self.process_token_via_insertion_mode(InsertionMode::InBody, token);
                    }
                }
            }
            InsertionMode::InColumnGroup => {
                handle_string_prefix!(token, b'\t' | b'\x0A' | b'\x0C' | b' ', |string| {
                    self.insert_a_character(string);
                });

                match token {
                    Some(Token::Comment(s)) => {
                        self.insert_a_comment(s, None);
                    }
                    Some(Token::Doctype(_)) => {
                        self.parse_error();
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"html") => {
                        self.process_token_via_insertion_mode(InsertionMode::InBody, token);
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"col") => {
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.stack_of_open_elements.pop();
                        // TODO: acknowledge self-closing flag
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"colgroup") => {
                        if !self.current_node().map_or(false, |node| node.is_element(b"colgroup")) {
                            self.parse_error();
                        } else {
                            self.stack_of_open_elements.pop();
                            self.insertion_mode = InsertionMode::InTable;
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"col") => {
                        self.parse_error();
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"template") => {
                        self.process_token_via_insertion_mode(InsertionMode::InHead, token);
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"template") => {
                        self.process_token_via_insertion_mode(InsertionMode::InHead, token);
                    }
                    None => {
                        self.process_token_via_insertion_mode(InsertionMode::InBody, token);
                    }
                    _ => {
                        if !self.current_node().map_or(false, |node| node.is_element(b"colgroup")) {
                            self.parse_error();
                        } else {
                            self.stack_of_open_elements.pop();
                            self.insertion_mode = InsertionMode::InTable;
                            self.reprocess_token(token);
                        }
                    }
                }
            }
            InsertionMode::InTableBody => {
                match token {
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"tr") => {
                        self.clear_stack_back_to_a_table_body_context();
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.insertion_mode = InsertionMode::InRow;
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"th" | b"td") => {
                        self.parse_error();
                        self.clear_stack_back_to_a_table_body_context();
                        self.insert_an_element_for_a_token(Token::StartTag(StartTag {
                            name: b"tr".as_slice().to_owned().into(),
                            ..StartTag::default()
                        }));
                        self.insertion_mode = InsertionMode::InRow;
                        self.reprocess_token(token);
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"tbody" | b"tfoot" | b"thead") => {
                        if !self.has_element_in_table_scope(&tag.name) {
                            self.parse_error();
                        } else {
                            self.clear_stack_back_to_a_table_body_context();
                            self.stack_of_open_elements.pop();
                            self.insertion_mode = InsertionMode::InTable;
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"caption" | b"col" | b"colgroup" | b"tbody" | b"tfoot" | b"thead") => {
                        if !self.has_element_in_table_scope(b"tbody") && !self.has_element_in_table_scope(b"thead") && !self.has_element_in_table_scope(b"tfoot") {
                            self.parse_error();
                        } else {
                            self.clear_stack_back_to_a_table_body_context();
                            self.stack_of_open_elements.pop();
                            self.insertion_mode = InsertionMode::InTable;
                            self.reprocess_token(token);
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"table") => {
                        if !self.has_element_in_table_scope(b"tbody") || !self.has_element_in_table_scope(b"thead") || !self.has_element_in_table_scope(b"tfoot") {
                            self.parse_error();
                        } else {
                            self.clear_stack_back_to_a_table_body_context();
                            self.stack_of_open_elements.pop();
                            self.insertion_mode = InsertionMode::InTable;
                            self.reprocess_token(token);
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"body" | b"caption" | b"col" | b"colgroup" | b"html" | b"td" | b"th" | b"tr") => {
                        self.parse_error();
                    }
                    _ => {
                        self.process_token_via_insertion_mode(InsertionMode::InTable, token);
                    }
                }
            }
            InsertionMode::InRow => {
                match token {
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"th" | b"td") => {
                        self.clear_stack_back_to_a_table_row_context();
                        self.insert_an_element_for_a_token(token.unwrap());
                        self.insertion_mode = InsertionMode::InCell;
                        self.list_of_active_formatting_elements.push(ElementOrMarker::Marker);
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"tr") => {
                        self.handle_in_row_inner(b"tr");
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"caption" | b"col" | b"colgroup" | b"tbody" | b"tfoot" | b"thead" | b"tr") => {
                        if self.handle_in_row_inner(b"tr") {
                            self.reprocess_token(token);
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"table") => {
                        if self.handle_in_row_inner(b"tr") {
                            self.reprocess_token(token);
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"tbody" | b"tfoot" | b"thead") => {
                        self.handle_in_row_inner(&tag.name);
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"body" | b"caption" | b"col" | b"colgroup" | b"html" | b"td" | b"th") => {
                        self.parse_error();
                    }
                    _ => {
                        self.process_token_via_insertion_mode(InsertionMode::InTable, token);
                    }
                }
            }
            InsertionMode::InCell => {
                match token {
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"td" | b"th") => {
                        if !self.has_element_in_table_scope(&tag.name) {
                            self.parse_error();
                        } else {
                            self.generate_implied_end_tags(&[]);
                            if !self.current_node().map_or(false, |node| node.is_element(&tag.name)) {
                                self.parse_error();
                            }

                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if node.is_element(&tag.name) {
                                    break;
                                }
                            }

                            self.clear_list_of_active_formatting_elements_up_to_the_last_marker();
                            self.insertion_mode = InsertionMode::InRow;
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"caption" | b"col" | b"colgroup" | b"tbody" | b"td" | b"tfoot" | b"th" | b"thead" | b"tr") => {
                        if !self.has_element_in_table_scope(b"td") && !self.has_element_in_table_scope(b"td") {
                            self.parse_error();
                        } else {
                            self.close_the_cell();
                            self.reprocess_token(token);
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"body" | b"caption" | b"col" | b"colgroup" | b"html") => {
                        self.parse_error();
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"table" | b"tbody" | b"tfoot" | b"thead" | b"tr") => {
                        if !self.has_element_in_table_scope(&tag.name) {
                            self.parse_error();
                        } else {
                            self.close_the_cell();
                            self.reprocess_token(token);
                        }
                    }
                    _ => {
                        self.process_token_via_insertion_mode(InsertionMode::InBody, token);
                    }
                }
            }
            InsertionMode::InSelect => {
                match token {
                    Some(Token::String(mut s)) => {
                        s.retain(|&c| {
                            if c == b'\0' {
                                self.parse_error();
                                false
                            } else {
                                true
                            }
                        });

                        if !s.is_empty() {
                            self.insert_a_character(&s);
                        }
                    }
                    Some(Token::Comment(s)) => {
                        self.insert_a_comment(s, None);
                    }
                    Some(Token::Doctype(_)) => {
                        self.parse_error();
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"html") => {
                        self.process_token_via_insertion_mode(InsertionMode::InBody, token);
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"option") => {
                        if self.current_node().map_or(false, |node| node.is_element(b"option")) {
                            self.stack_of_open_elements.pop();
                        }

                        self.insert_an_element_for_a_token(token.unwrap());
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"optgroup") => {
                        if self.current_node().map_or(false, |node| node.is_element(b"option")) {
                            self.stack_of_open_elements.pop();
                        }

                        if self.current_node().map_or(false, |node| node.is_element(b"optgroup")) {
                            self.stack_of_open_elements.pop();
                        }

                        self.insert_an_element_for_a_token(token.unwrap());
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"optgroup") => {
                        if self.current_node().map_or(false, |node| node.is_element(b"option")) && self.stack_of_open_elements.get(self.stack_of_open_elements.len() - 2).map_or(false, |node| node.is_element(b"optgroup")) {
                            self.stack_of_open_elements.pop();
                        }

                        if self.current_node().map_or(false, |node| node.is_element(b"optgroup")) {
                            self.stack_of_open_elements.pop();
                        } else {
                            self.parse_error();
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"option") => {
                        if self.current_node().map_or(false, |node| node.is_element(b"option")) {
                            self.stack_of_open_elements.pop();
                        } else {
                            self.parse_error();
                        }
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"select") => {
                        if !self.has_element_in_select_scope(b"select") {
                            self.parse_error();
                        } else {
                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if node.is_element(b"select") {
                                    break;
                                }
                            }

                            self.reset_the_insertion_mode_appropriately();
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"select") => {
                        self.parse_error();
                        if self.has_element_in_select_scope(b"select") {
                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if node.is_element(b"select") {
                                    break;
                                }
                            }

                            self.reset_the_insertion_mode_appropriately();
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"input" | b"keygen" | b"textarea") => {
                        self.parse_error();
                        if self.has_element_in_select_scope(b"select") {
                            while let Some(node) = self.stack_of_open_elements.pop() {
                                if node.is_element(b"select") {
                                    break;
                                }
                            }

                            self.reset_the_insertion_mode_appropriately();
                            self.reprocess_token(token);
                        }
                    }
                    Some(Token::StartTag(ref tag)) if matches!(tag.name.as_slice(), b"script" | b"template") => {
                        self.process_token_via_insertion_mode(InsertionMode::InHead, token);
                    }
                    Some(Token::EndTag(ref tag)) if matches!(tag.name.as_slice(), b"template") => {
                        self.process_token_via_insertion_mode(InsertionMode::InHead, token);
                    }
                    None => {
                        self.process_token_via_insertion_mode(InsertionMode::InBody, token);
                    }
                    _ => {
                        self.parse_error();
                    }
                }
            }
            _ => todo!()
        }
    }

    fn handle_in_row_inner(&mut self, tag_for_scope: &[u8]) ->  bool {
        if !self.has_element_in_table_scope(tag_for_scope) {
            self.parse_error();
            false
        } else if tag_for_scope != b"tr" && !self.has_element_in_table_scope(b"tr") {
            false
        } else {
            self.clear_stack_back_to_a_table_row_context();
            self.stack_of_open_elements.pop();
            self.insertion_mode = InsertionMode::InTableBody;
            true
        }
    }

    fn handle_in_caption_inner(&mut self) -> bool {
        if !self.has_element_in_table_scope(b"caption") {
            self.parse_error();
            false
        } else {
            self.generate_implied_end_tags(&[]);
            if self.current_node().map_or(false, |node| node.is_element(b"caption")) {
                self.parse_error();
            }

            while let Some(node) = self.stack_of_open_elements.pop() {
                if node.is_element(b"caption") {
                    break;
                }
            }

            self.clear_list_of_active_formatting_elements_up_to_the_last_marker();
            self.insertion_mode = InsertionMode::InTable;
            true
        }
    }

    fn has_element_in_scope(&self, name: &[u8]) -> bool {
        todo!()
    }

    fn has_element_in_scope2(&self, matcher: impl Fn(&Node) -> bool) -> bool {
        todo!()
    }

    fn has_element_in_button_scope(&self, name: &[u8]) -> bool {
        todo!()
    }

    fn has_element_in_list_item_scope(&self, name: &[u8]) -> bool {
        todo!()
    }

    fn has_element_in_table_scope(&self, name: &[u8]) -> bool {
        todo!()
    }

    fn has_element_in_select_scope(&self, name: &[u8]) -> bool {
        todo!()
    }

    fn close_a_p_element(&mut self) {
        todo!()
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

    fn reconstruct_the_active_formatting_elements(&mut self) {
        todo!()
    }

    fn stop_parsing(&mut self) {
        todo!()
    }

    fn peek_token(&mut self) -> &mut Option<Token> {
        todo!()
    }

    fn generate_implied_end_tags(&mut self, except_for_tags: &[&[u8]]) {
        todo!()
    }

    fn run_adoption_agency_algorithm(&mut self, token: Token) {
        todo!()
    }

    fn adjust_mathml_attributes(&mut self, token: &mut Token) {
        todo!()
    }

    fn adjust_foreign_attributes(&mut self, token: &mut Token) {
        todo!()
    }

    fn adjust_svg_attributes(&mut self, token: &mut Token) {
        todo!()
    }

    fn insert_a_foreign_element(&mut self, token: Token, namespace: ElementNamespace) {
        todo!()
    }

    fn clear_stack_back_to_a_table_context(&mut self) {
        todo!()
    }

    fn clear_stack_back_to_a_table_body_context(&mut self) {
        todo!()
    }

    fn clear_stack_back_to_a_table_row_context(&mut self) {
        todo!()
    }

    fn reprocess_token(&mut self, token: Option<Token>) {
        self.process_token_via_insertion_mode(self.insertion_mode, token);
    }
    
    fn close_the_cell(&mut self) {
        self.generate_implied_end_tags(&[]);
        if !self.current_node().map_or(false, |node| node.is_element(b"td") || node.is_element(b"th")) {
            self.parse_error();
        }

        while let Some(node) = self.stack_of_open_elements.pop() {
            if node.is_element(b"td") || node.is_element(b"th") {
                break;
            }
        }
        self.clear_list_of_active_formatting_elements_up_to_the_last_marker();
        self.insertion_mode = InsertionMode::InRow;
    }
}
