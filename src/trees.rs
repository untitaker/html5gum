use std::io;
use std::rc::Rc;

use crate::{Doctype as DoctypeToken, Reader, Token, Tokenizer};

struct Comment {
    data: String,
    // missing: node document
}

enum QuirksMode {
    True,
    False,
    Limited,
}

struct Element {
    name: String,
}

struct Document {
    children: Vec<Rc<Node>>,
    quirks_mode: QuirksMode,
}

struct Doctype {
    name: String,
    public_identifier: String,
    system_identifier: String,
}

enum Node {
    Element(Element),
    Document(Document),
    Comment(Comment),
    Doctype(Doctype),
}

#[derive(Copy, Eq, Clone, Debug, PartialEq)]
enum InsertionMode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InBody,
    InHead,
}

struct TreeBuilder<R: Reader> {
    open_elements_stack: Vec<Rc<Node>>,
    head_element: Option<Rc<Node>>,
    form_element: Option<Rc<Node>>,
    tokenizer: Tokenizer<R>,
    mode: InsertionMode,
    document: Document,
    parser_cannot_change_mode: bool,
}

enum Location {
    DocumentLast,
}

macro_rules! ignore_whitespace {
    ($token:expr) => {
        if let Token::String(ref mut string) = $token {
            string.retain(|x| x != '\t' && x != '\n' && x != ' ');
            if string.is_empty() {
                return Ok(());
            }
        }
    };
}

impl<R: Reader> TreeBuilder<R> {
    pub fn new(tokenizer: Tokenizer<R>) -> Self {
        TreeBuilder {
            open_elements_stack: Vec::new(),
            tokenizer,
            mode: InsertionMode::Initial,
            document: Document {
                children: Vec::new(),
                quirks_mode: QuirksMode::False,
            },
            parser_cannot_change_mode: false,
            form_element: None,
            head_element: None,
        }
    }

    fn appropriate_place_for_inserting_a_node(&self) -> Location {
        todo!()
    }

    fn insert(&mut self, location: Option<Location>, node: Rc<Node>) {
        match location {
            Some(Location::DocumentLast) => self.document.children.push(node),
            _ => todo!(),
        }
    }

    fn insert_foreign_element(&mut self, token: Token) {
        let adjusted_insertion_location = self.appropriate_place_for_inserting_a_node();
        todo!()
    }

    fn insert_html_element(&mut self, token: Token) {
        // TODO: Specify namespace
        self.insert_foreign_element(token)
    }

    fn process_token_using_the_rules_for(&mut self, mode: InsertionMode, token: Token) -> Result<(), io::Error> {
        let mode_bak = self.mode;
        self.mode = mode;
        let rv = self.process_token(token);
        self.mode = mode_bak;
        rv
    }

    fn process_token(&mut self, mut token: Token) -> Result<(), io::Error> {
        match self.mode {
            InsertionMode::Initial => {
                ignore_whitespace!(token);

                match token {
                    Token::Comment(data) => {
                        self.insert(
                            Some(Location::DocumentLast),
                            Rc::new(Node::Comment(Comment { data })),
                        );
                        Ok(())
                    }
                    Token::Doctype(doctype) => {
                        if doctype.name != "html"
                            || doctype.public_identifier.is_some()
                            || matches!(doctype.system_identifier, Some(ref x) if x != "about:legacy-compat")
                        {
                            // err: ???
                        }

                        // missing: we assume we are NOT an iframe srcdoc document
                        if !self.parser_cannot_change_mode && doctype_is_quirky(&doctype) {
                            self.document.quirks_mode = QuirksMode::True;
                        } else if !self.parser_cannot_change_mode && doctype_is_limited_quirky(&doctype) {
                            self.document.quirks_mode = QuirksMode::Limited;
                        }

                        self.insert(
                            Some(Location::DocumentLast),
                            Rc::new(Node::Doctype(Doctype {
                                name: doctype.name,
                                public_identifier: doctype.public_identifier.unwrap_or_default(),
                                system_identifier: doctype.system_identifier.unwrap_or_default(),
                            })),
                        );

                        self.mode = InsertionMode::BeforeHtml;
                        Ok(())
                    }
                    x => {
                        // err: ???
                        self.mode = InsertionMode::BeforeHtml;
                        self.process_token(x)
                    }
                }
            }
            InsertionMode::BeforeHtml => {
                ignore_whitespace!(token);

                match token {
                    Token::Doctype(_) => {
                        // err: ???
                        Ok(())
                    }
                    Token::Comment(data) => {
                        self.insert(
                            Some(Location::DocumentLast),
                            Rc::new(Node::Comment(Comment { data })),
                        );
                        Ok(())
                    }
                    Token::StartTag(tag) if tag.name == "html" => {
                        let elem = Element {
                            name: tag.name,
                        };

                        let node = Rc::new(Node::Element(elem));

                        self.insert(
                            Some(Location::DocumentLast),
                            node.clone(),
                        );

                        self.open_elements_stack.push(node);
                        self.mode = InsertionMode::BeforeHead;
                        Ok(())
                    }
                    Token::EndTag(tag) if tag.name != "head" && tag.name != "body" && tag.name != "html" && tag.name != "br" => {
                        // err: ???
                        Ok(())
                    }
                    x => {
                        let node = Rc::new(Node::Element(Element { name: x.name().unwrap().to_owned() }));
                        self.insert(Some(Location::DocumentLast), node.clone());
                        self.open_elements_stack.push(node);
                        self.process_token(x)
                    }
                }
            }

            InsertionMode::BeforeHead => {
                ignore_whitespace!(token);

                match token {
                    Token::Comment(data) => {
                        self.insert(Some(self.appropriate_place_for_inserting_a_node()), Rc::new(Node::Comment(Comment { data })));
                        Ok(())
                    }
                    Token::Doctype(_doctype) => {
                        // err: ???
                        Ok(())
                    }
                    Token::StartTag(tag) if tag.name == "html" => {
                        self.process_token_using_the_rules_for(InsertionMode::InBody, Token::StartTag(tag))
                    }
                    Token::StartTag(tag) if tag.name == "head" => {
                        let element = Rc::new(Node::Element(Element { name: "head".to_owned() }));
                        self.insert(Some(self.appropriate_place_for_inserting_a_node()), element.clone());
                        self.head_element = Some(element);
                        self.mode = InsertionMode::InHead;
                        Ok(())
                    }
                    Token::EndTag(tag) if tag.name != "head" && tag.name != "body" && tag.name != "html" && tag.name != "br" => {
                        // "any other end tag"
                        // err: ???
                        Ok(())
                    }
                    _ => {
                        // "anything else" + "any end tag whose name is one of ..."
                        let element = Rc::new(Node::Element(Element { name: "head".to_owned() }));
                        self.insert(Some(self.appropriate_place_for_inserting_a_node()), element.clone());
                        self.head_element = Some(element);
                        self.mode = InsertionMode::InHead;
                        self.process_token(token)
                    }
                }
            }
            _ => todo!()
        }
    }
}

fn starts_with_ignore_ascii_case(haystack: &str, prefix: &str) -> bool {
    if let Some(prefix2) = haystack.get(..prefix.len()) {
        prefix2.eq_ignore_ascii_case(prefix)
    } else {
        false
    }
}

fn doctype_is_limited_quirky(doctype: &DoctypeToken) -> bool {
    let public = doctype.public_identifier.as_ref().map(String::as_str).unwrap_or_default();
    starts_with_ignore_ascii_case(public, "-//W3C//DTD XHTML 1.0 Frameset//") || 
    starts_with_ignore_ascii_case(public, "-//W3C//DTD XHTML 1.0 Transitional//") || (doctype.system_identifier.is_some() && starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML 4.01 Frameset//")) || (doctype.system_identifier.is_some() && starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML 4.01 Transitional//"))
}

fn doctype_is_quirky(doctype: &DoctypeToken) -> bool {
    let public = doctype.public_identifier.as_ref().map(String::as_str).unwrap_or_default();
    let system = doctype.system_identifier.as_ref().map(String::as_str).unwrap_or_default();
    let name = &doctype.name;

    doctype.force_quirks
        || name != "html"
        || public == "-//W3O//DTD W3 HTML Strict 3.0//EN//"
        || public == "HTML"
        || system == "http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd"
        || starts_with_ignore_ascii_case(public, "+//Silmaril//dtd html Pro v0r11 19970101//")
        || starts_with_ignore_ascii_case(public, "-//AS//DTD HTML 3.0 asWedit + extensions//")
        || starts_with_ignore_ascii_case(public, "-//AdvaSoft Ltd//DTD HTML 3.0 asWedit + extensions//")
        || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML 2.0 Level 1//")
        || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML 2.0 Level 2//")
        || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML 2.0 Strict Level 1//")
        || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML 2.0 Strict Level 2//")
        || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML 2.0 Strict//")
        || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML 2.0//")
        || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML 2.1E//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML 3.0//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML 3.2 Final//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML 3.2//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML 3//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML Level 0//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML Level 1//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML Level 2//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML Level 3//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML Strict Level 0//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML Strict Level 1//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML Strict Level 2//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML Strict Level 3//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML Strict//") || starts_with_ignore_ascii_case(public, "-//IETF//DTD HTML//") || starts_with_ignore_ascii_case(public, "-//Metrius//DTD Metrius Presentational//") || starts_with_ignore_ascii_case(public, "-//Microsoft//DTD Internet Explorer 2.0 HTML Strict//") || starts_with_ignore_ascii_case(public, "-//Microsoft//DTD Internet Explorer 2.0 HTML//") || starts_with_ignore_ascii_case(public, "-//Microsoft//DTD Internet Explorer 2.0 Tables//") || starts_with_ignore_ascii_case(public, "-//Microsoft//DTD Internet Explorer 3.0 HTML Strict//") || starts_with_ignore_ascii_case(public, "-//Microsoft//DTD Internet Explorer 3.0 HTML//") || starts_with_ignore_ascii_case(public, "-//Microsoft//DTD Internet Explorer 3.0 Tables//") || starts_with_ignore_ascii_case(public, "-//Netscape Comm. Corp.//DTD HTML//") || starts_with_ignore_ascii_case(public, "-//Netscape Comm. Corp.//DTD Strict HTML//") || starts_with_ignore_ascii_case(public, "-//O'Reilly and Associates//DTD HTML 2.0//") || starts_with_ignore_ascii_case(public, "-//O'Reilly and Associates//DTD HTML Extended 1.0//") || starts_with_ignore_ascii_case(public, "-//O'Reilly and Associates//DTD HTML Extended Relaxed 1.0//") || starts_with_ignore_ascii_case(public, "-//SQ//DTD HTML 2.0 HoTMetaL + extensions//") || starts_with_ignore_ascii_case(public, "-//SoftQuad Software//DTD HoTMetaL PRO 6.0::19990601::extensions to HTML 4.0//") || starts_with_ignore_ascii_case(public, "-//SoftQuad//DTD HoTMetaL PRO 4.0::19971010::extensions to HTML 4.0//") || starts_with_ignore_ascii_case(public, "-//Spyglass//DTD HTML 2.0 Extended//") || starts_with_ignore_ascii_case(public, "-//Sun Microsystems Corp.//DTD HotJava HTML//") || starts_with_ignore_ascii_case(public, "-//Sun Microsystems Corp.//DTD HotJava Strict HTML//") || starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML 3 1995-03-24//") || starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML 3.2 Draft//") || starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML 3.2 Final//") || starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML 3.2//") || starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML 3.2S Draft//") || starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML 4.0 Frameset//") || starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML 4.0 Transitional//") || starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML Experimental 19960712//") || starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML Experimental 970421//") || starts_with_ignore_ascii_case(public, "-//W3C//DTD W3 HTML//") || starts_with_ignore_ascii_case(public, "-//W3O//DTD W3 HTML 3.0//") || starts_with_ignore_ascii_case(public, "-//WebTechs//DTD Mozilla HTML 2.0//") || starts_with_ignore_ascii_case(public, "-//WebTechs//DTD Mozilla HTML//" ) || (
            doctype.system_identifier.is_none() && starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML 4.01 Frameset//")
        ) || (
        doctype.system_identifier.is_none() && starts_with_ignore_ascii_case(public, "-//W3C//DTD HTML 4.01 Transitional//")
        )
}
