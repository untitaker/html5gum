#[macro_use] extern crate html5ever;

use std::{iter::repeat, collections::HashMap};

use html5gum::{Html5everEmitter, IoReader, Tokenizer};
use html5ever::tree_builder::{
    AppendNode, AppendText, ElementFlags, NodeOrText, QuirksMode, TreeSink,
};
use html5ever::{Attribute, ExpandedName, QualName};
use html5ever::tokenizer::{CharacterTokens, EndTag, NullCharacterToken, StartTag, TagToken};
use html5ever::tokenizer::{
    ParseError, Token, TokenSink, TokenSinkResult, TokenizerOpts,
};
use html5ever::tendril::*;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

#[derive(Copy, Clone)]
struct TokenPrinter {
    in_char_run: bool,
}

impl TokenPrinter {
    fn is_char(&mut self, is_char: bool) {
        match (self.in_char_run, is_char) {
            (false, true) => print!("CHAR : \""),
            (true, false) => println!("\""),
            _ => (),
        }
        self.in_char_run = is_char;
    }

    fn do_char(&mut self, c: char) {
        self.is_char(true);
        print!("{}", c.escape_default().collect::<String>());
    }
}

impl TokenSink for TokenPrinter {
    type Handle = ();

    fn process_token(&mut self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        match token {
            CharacterTokens(b) => {
                for c in b.chars() {
                    self.do_char(c);
                }
            },
            NullCharacterToken => self.do_char('\0'),
            TagToken(tag) => {
                self.is_char(false);
                // This is not proper HTML serialization, of course.
                match tag.kind {
                    StartTag => print!("TAG  : <\x1b[32m{}\x1b[0m", tag.name),
                    EndTag => print!("TAG  : <\x1b[31m/{}\x1b[0m", tag.name),
                }
                for attr in tag.attrs.iter() {
                    print!(
                        " \x1b[36m{}\x1b[0m='\x1b[34m{}\x1b[0m'",
                        attr.name.local, attr.value
                    );
                }
                if tag.self_closing {
                    print!(" \x1b[31m/\x1b[0m");
                }
                println!(">");
            },
            ParseError(err) => {
                self.is_char(false);
                println!("ERROR: {}", err);
            },
            _ => {
                self.is_char(false);
                println!("OTHER: {:?}", token);
            },
        }
        TokenSinkResult::Continue
    }
}

fn main() {
    let mut sink = TokenPrinter { in_char_run: false };
    let token_emitter = Html5everEmitter::new(sink);

    let tokenizer =
        Tokenizer::new_with_emitter(IoReader::new(std::io::stdin().lock()), token_emitter);

    for result in tokenizer {
        result.unwrap();
    }
}
