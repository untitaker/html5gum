use std::{collections::BTreeMap, fs::File, io::BufReader, path::Path};

use html5gum::{
    Doctype, EndTag, Error, IoReader, Readable, Reader, Span, Spanned, StartTag, State, Token,
    Tokenizer,
};

use html5gum::testutils::{trace_log, SlowReader};

use glob::glob;
use libtest_mimic::{self, Arguments, Failed, Trial};
use pretty_assertions::assert_eq;
use serde::{de::Error as _, Deserialize};

mod testutils;

#[derive(Clone)]
struct ExpectedOutputTokens(Vec<Token<()>>);

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
                    ) => Token::Doctype(Spanned {
                        value: Doctype {
                            name: name.unwrap_or_default().0.into(),
                            public_identifier: public_identifier.map(|x| x.0.into()),
                            system_identifier: system_identifier.map(|x| x.0.into()),
                            force_quirks: !correctness,
                        },
                        span: Span::DUMMY,
                    }),
                    OutputToken::StartTag(_, name, attributes) => Token::StartTag(StartTag {
                        self_closing: false,
                        name: name.0.into(),
                        attributes: attributes
                            .into_iter()
                            .map(|(k, v)| {
                                (
                                    k.0.into(),
                                    Spanned {
                                        value: v.0.into(),
                                        span: Span::DUMMY,
                                    },
                                )
                            })
                            .collect(),
                        span: Span::DUMMY,
                    }),
                    OutputToken::StartTag2(_, name, attributes, self_closing) => {
                        Token::StartTag(StartTag {
                            self_closing,
                            name: name.0.into(),
                            attributes: attributes
                                .into_iter()
                                .map(|(k, v)| {
                                    (
                                        k.0.into(),
                                        Spanned {
                                            value: v.0.into(),
                                            span: Span::DUMMY,
                                        },
                                    )
                                })
                                .collect(),
                            span: Span::DUMMY,
                        })
                    }
                    OutputToken::EndTag(_, name) => Token::EndTag(EndTag {
                        name: name.0.into(),
                        span: Span::DUMMY,
                    }),
                    OutputToken::Comment(_, data) => Token::Comment(Spanned {
                        value: data.0.into(),
                        span: Span::DUMMY,
                    }),
                    OutputToken::Character(_, data) => Token::String(Spanned {
                        value: data.0.into(),
                        span: Span::DUMMY,
                    }),
                })
                .collect::<Vec<Token<()>>>(),
        ))
    }
}

#[derive(Clone)]
struct InitialState(State);

impl<'de> Deserialize<'de> for InitialState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        enum RawInitialState {
            #[serde(rename = "Data state")]
            Data,
            #[serde(rename = "PLAINTEXT state")]
            PlainText,
            #[serde(rename = "RCDATA state")]
            RcData,
            #[serde(rename = "RAWTEXT state")]
            RawText,
            #[serde(rename = "Script data state")]
            ScriptData,
            #[serde(rename = "CDATA section state")]
            CdataSection,
        }

        Ok(Self(match RawInitialState::deserialize(deserializer)? {
            RawInitialState::Data => State::Data,
            RawInitialState::PlainText => State::PlainText,
            RawInitialState::RcData => State::RcData,
            RawInitialState::RawText => State::RawText,
            RawInitialState::ScriptData => State::ScriptData,
            RawInitialState::CdataSection => State::CdataSection,
        }))
    }
}

fn initial_states_default() -> Vec<InitialState> {
    vec![InitialState(State::Data)]
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TestFileEntry {
    description: String,
    input: HtmlString,
    output: ExpectedOutputTokens,
    #[serde(default = "initial_states_default")]
    initial_states: Vec<InitialState>,
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

#[derive(Deserialize)]
struct TestFile {
    tests: Vec<TestFileEntry>,
}

struct TestCase {
    state: State,
    reader_type: ReaderType,
    filename: String,
    test_i: usize,
    declaration: TestFileEntry,
}

impl TestCase {
    fn run(&self) -> Result<(), Failed> {
        testutils::catch_unwind_and_report(move || {
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
        })
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
                    code: ParseErrorInner(*e),
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

fn produce_testcases_from_file(tests: &mut Vec<Trial>, path: &Path) {
    let fname = path.file_name().unwrap().to_str().unwrap();

    if matches!(
        fname,
        // We don't implement "Coercing an HTML DOM into an infoset" section
        "xmlViolation.test"
        // We don't detect surrogates
        | "unicodeCharsProblematic.test"
    ) {
        return;
    }

    let f = File::open(path).unwrap();
    let bf = BufReader::new(f);
    let TestFile {
        tests: declarations,
    } = serde_json::from_reader(bf).unwrap();

    for (test_i, mut declaration) in declarations.into_iter().enumerate() {
        if declaration.double_escaped {
            declaration.input.0 = unescape(&declaration.input.0);

            declaration.output.0 = declaration
                .output
                .0
                .into_iter()
                .map(|token| match token {
                    Token::String(x) => Token::String(Spanned {
                        value: unescape(x.as_slice()).into(),
                        span: Span::DUMMY,
                    }),
                    Token::Comment(x) => Token::Comment(Spanned {
                        value: unescape(x.as_slice()).into(),
                        span: Span::DUMMY,
                    }),
                    token => token,
                })
                .collect();
        }

        for state in &declaration.initial_states {
            for &reader_type in &[
                ReaderType::SlowString,
                ReaderType::String,
                ReaderType::BufRead,
                ReaderType::SlowBufRead,
            ] {
                let filename = fname.to_owned();
                let declaration = declaration.clone();
                let state = state.0;
                tests.push(Trial::test(
                    format!(
                        "{}:{}:{:?}:{:?}",
                        fname, declaration.description, state, reader_type
                    ),
                    move || {
                        TestCase {
                            state,
                            reader_type,
                            filename,
                            test_i,
                            declaration,
                        }
                        .run()
                    },
                ));
            }
        }
    }
}

fn main() {
    let args = Arguments::from_args();

    let mut tests = Vec::new();

    for entry in glob("tests/html5lib-tests/tokenizer/*.test").unwrap() {
        produce_testcases_from_file(&mut tests, &entry.unwrap());
    }

    for entry in glob("tests/custom-html5lib-tests/tokenizer/*.test").unwrap() {
        produce_testcases_from_file(&mut tests, &entry.unwrap());
    }

    libtest_mimic::run(&args, tests).exit();
}
