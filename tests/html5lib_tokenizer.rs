use std::ops::Deref;
use std::{collections::BTreeMap};

use html5gum::{
    Doctype, EndTag, Error, IoReader, Readable, Reader, StartTag, State, Token, Tokenizer,
};

use html5gum::testutils::{trace_log, SlowReader, OUTPUT};

use pretty_assertions::assert_eq;
use serde::{de::Error as _, Deserialize};

#[derive(Clone)]
struct ExpectedOutputTokens(Vec<Token>);

#[derive(Deserialize, Ord, PartialOrd, PartialEq, Eq, Default, Clone)]
struct HtmlString(#[serde(with = "serde_bytes")] Vec<u8>);

impl<'de> Deserialize<'de> for ExpectedOutputTokens {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // this macro is a horrible way to define a type that deserializes only from a particular
        // string. Together with serde(untagged) this gives us really flexible enum tagging with really
        // terrible error messages.
        macro_rules! def_const {
            ($str:expr, $ty:ident) => {
                #[derive(Deserialize)]
                enum $ty {
                    #[serde(rename = $str)]
                    $ty,
                }
            };
        }

        def_const!("DOCTYPE", DoctypeConst);
        def_const!("StartTag", StartTagConst);
        def_const!("EndTag", EndTagConst);
        def_const!("Comment", CommentConst);
        def_const!("Character", CharacterConst);

        type Attributes = BTreeMap<HtmlString, HtmlString>;

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OutputToken {
            // "DOCTYPE", name, public_id, system_id, correctness
            Doctype(
                DoctypeConst,
                Option<HtmlString>,
                Option<HtmlString>,
                Option<HtmlString>,
                bool,
            ),
            // "StartTag", name, attributes, self_closing
            StartTag(StartTagConst, HtmlString, Attributes),
            StartTag2(StartTagConst, HtmlString, Attributes, bool),
            // "EndTag", name
            EndTag(EndTagConst, HtmlString),
            // "Comment", data
            Comment(CommentConst, HtmlString),
            // "Character", data
            Character(CharacterConst, HtmlString),
        }

        Ok(ExpectedOutputTokens(
            Vec::deserialize(deserializer)?
                .into_iter()
                .map(|output_token| match output_token {
                    OutputToken::Doctype(
                        _,
                        name,
                        public_identifier,
                        system_identifier,
                        correctness,
                    ) => Token::Doctype(Doctype {
                        name: name.unwrap_or_default().0.into(),
                        public_identifier: public_identifier.map(|x| x.0.into()),
                        system_identifier: system_identifier.map(|x| x.0.into()),
                        force_quirks: !correctness,
                    }),
                    OutputToken::StartTag(_, name, attributes) => Token::StartTag(StartTag {
                        self_closing: false,
                        name: name.0.into(),
                        attributes: attributes
                            .into_iter()
                            .map(|(k, v)| (k.0.into(), v.0.into()))
                            .collect(),
                    }),
                    OutputToken::StartTag2(_, name, attributes, self_closing) => {
                        Token::StartTag(StartTag {
                            self_closing,
                            name: name.0.into(),
                            attributes: attributes
                                .into_iter()
                                .map(|(k, v)| (k.0.into(), v.0.into()))
                                .collect(),
                        })
                    }
                    OutputToken::EndTag(_, name) => Token::EndTag(EndTag {
                        name: name.0.into(),
                    }),
                    OutputToken::Comment(_, data) => Token::Comment(data.0.into()),
                    OutputToken::Character(_, data) => Token::String(data.0.into()),
                })
                .collect::<Vec<Token>>(),
        ))
    }
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TestFileEntry {
    description: String,
    input: HtmlString,
    output: ExpectedOutputTokens,
    #[serde(default)]
    double_escaped: bool,
    #[serde(default)]
    last_start_tag: Option<String>,
    #[serde(default)]
    errors: Vec<ParseError>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
struct ParseErrorInner(Error);


impl<'de> Deserialize<'de> for ParseErrorInner {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let str_err = String::deserialize(deserializer)?;
        let err: Error = str_err
            .parse()
            .map_err(|_| D::Error::custom(format!("failed to deserialize error: {}", str_err)))?;
        Ok(ParseErrorInner(err))
    }
}

#[derive(Deserialize, Debug, Eq, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
struct ParseError {
    code: ParseErrorInner,
}

struct TestCase {
    state: State,
    reader_type: ReaderType,
    filename: String,
    test_i: usize,
    declaration: TestFileEntry,
}

impl TestCase {
    fn run(mut self) {
        if self.declaration.double_escaped {
            self.declaration.input.0 = unescape(&self.declaration.input.0);

            self.declaration.output.0 = self.declaration
                .output
                .0
                .into_iter()
                .map(|token| match token {
                    Token::String(x) => Token::String(unescape(x.as_slice()).into()),
                    Token::Comment(x) => Token::Comment(unescape(x.as_slice()).into()),
                    token => token,
                })
                .collect();
        }

        let result = std::panic::catch_unwind(move || {
            trace_log(&format!(
                "==== FILE {}, TEST {}, STATE {:?}, TOKENIZER {:?} ====",
                self.filename, self.test_i, self.state, self.reader_type,
            ));
            trace_log(&format!("description: {}", self.declaration.description));

            let string = self.declaration.input.0.as_slice();

            match self.reader_type {
                ReaderType::String => self.run_inner(Tokenizer::new(string.to_reader())),
                ReaderType::SlowString => {
                    self.run_inner(Tokenizer::new(SlowReader(string.to_reader())));
                }
                ReaderType::BufRead => self.run_inner(Tokenizer::new(IoReader::new(string))),
                ReaderType::SlowBufRead => self.run_inner(Tokenizer::new(SlowReader(
                    IoReader::new(string).to_reader(),
                ))),
            }
        });

        if let Err(e) = result {
            let mut msg = String::new();

            OUTPUT.with(|cell| {
                let mut buf = cell.take();
                msg.push_str(&buf);
                buf.clear();
                cell.set(buf);
            });

            msg.push('\n');
            if let Some(s) = e
                // Try to convert it to a String, then turn that into a str
                .downcast_ref::<String>()
                    .map(String::as_str)
                    // If that fails, try to turn it into a &'static str
                    .or_else(|| e.downcast_ref::<&'static str>().map(Deref::deref))
            {
                msg.push_str(s);
            }

            panic!("{}", msg);
        }
    }

    fn run_inner<R: Reader>(&self, mut tokenizer: Tokenizer<R>) {
        tokenizer.set_state(self.state);
        tokenizer.set_last_start_tag(self.declaration.last_start_tag.as_deref());

        let mut actual_tokens = Vec::new();
        let mut actual_errors = Vec::new();

        for token in tokenizer {
            let token = token.unwrap();

            if let Token::Error(e) = token {
                actual_errors.push(ParseError {
                    code: ParseErrorInner(e),
                });
            } else {
                actual_tokens.push(token);
            }
        }

        assert_eq!(actual_tokens, self.declaration.output.0);
        assert_eq!(actual_errors, self.declaration.errors);
    }
}

#[derive(Debug, Clone, Copy)]
enum ReaderType {
    SlowString,
    String,
    BufRead,
    SlowBufRead,
}

/// Implements the escape sequences described in the tokenizer tests of html5lib-tests (and nothing
/// more)
fn unescape(data: &[u8]) -> Vec<u8> {
    let mut stream = data.iter();
    let mut rv = Vec::new();

    loop {
        match stream.next() {
            Some(b'\\') => (),
            Some(x) => {
                rv.push(*x);
                continue;
            }
            None => break,
        }

        match stream.next() {
            Some(b'u') => (),
            x => panic!("unexpected escape: {:?}", x),
        }

        let orig_len = rv.len();

        for _ in 0..4 {
            rv.push(match stream.next() {
                Some(x) => *x,
                None => panic!("unexpected eof after \\u"),
            });
        }

        let c = u32::from_str_radix(std::str::from_utf8(&rv[orig_len..]).unwrap(), 16)
            .expect("failed to parse as hex");
        rv.truncate(orig_len);

        if let Some(c) = char::from_u32(c) {
            rv.push(0);
            rv.push(0);
            rv.push(0);
            rv.push(0);
            let char_len = c.encode_utf8(&mut rv[orig_len..]).len();
            rv.truncate(orig_len + char_len);
        } else if (0xD800..=0xDFFF).contains(&c) {
            // a surrogate
            for b in &c.to_be_bytes()[2..] {
                rv.push(*b);
            }
        }
    }

    rv
}

script_macro::run_script!(r###"
    fn convert_state_enum(state) {
        let state_enum = #{
            "Data state": "State::Data",
            "PLAINTEXT state": "State::PlainText",
            "RCDATA state": "State::RcData",
            "RAWTEXT state": "State::RawText",
            "Script data state": "State::ScriptData",
            "CDATA section state": "State::CdataSection"
        };
        return state_enum[state];
    }

    fn produce_testcases_from_file(entry) {
        let output = "";
        let fname = basename(entry);

        // We don't implement "Coercing an HTML DOM into an infoset" section
        if fname == "xmlViolation.test" {
            return;
        }

        // We don't detect surrogates
        if fname == "unicodeCharsProblematic.test" {
            return;
        }

        let test_i = 0;
        for declaration in parse_json(open_file(entry).read_string())["tests"] {
            for state in declaration["initialStates"] ?? ["Data state"] {
                for reader_type in ["SlowString", "String", "BufRead", "SlowBufRead"] {
                    let test_name = slugify_ident(`test_${fname}${declaration["description"]}:${state}:${reader_type}:${test_i}`);
                    let state_enum = convert_state_enum(state);

                    output += `
                        #[test]
                        fn ${test_name}() {
                            TestCase {
                                state: ${state_enum},
                                reader_type: ReaderType::${reader_type},
                                filename: "${fname}".to_string(),
                                test_i: ${test_i},
                                declaration: serde_json::from_str(r##"${stringify_json(declaration)}"##).unwrap(),
                            }.run();
                        }
                    `;
                }
            }
            test_i += 1;
        }

        return output;
    }

    let output = "";

    for entry in glob("tests/html5lib-tests/tokenizer/*.test") {
        output += produce_testcases_from_file(entry);
    }

    for entry in glob("tests/custom-html5lib-tests/*.test") {
        output += produce_testcases_from_file(entry);
    }

    return output;
"###);
