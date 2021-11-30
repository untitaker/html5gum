use html5gum::{
    Doctype, EndTag, Error, InternalState as State, Reader, StartTag, Token, Tokenizer,
};
use pretty_assertions::assert_eq;
use serde::{de::Error as _, Deserialize};
use std::{collections::BTreeMap, fs::File, io::BufReader, path::Path};

#[cfg(not(feature = "integration-tests"))]
compile_error!(
    "integration tests need the integration-tests feature enabled. Run cargo tests --all-features"
);

struct ExpectedOutputTokens(Vec<Token>);

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

        type Attributes = BTreeMap<String, String>;

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OutputToken {
            // "DOCTYPE", name, public_id, system_id, correctness
            Doctype(
                DoctypeConst,
                Option<String>,
                Option<String>,
                Option<String>,
                bool,
            ),
            // "StartTag", name, attributes, self_closing
            StartTag(StartTagConst, String, Attributes),
            StartTag2(StartTagConst, String, Attributes, bool),
            // "EndTag", name
            EndTag(EndTagConst, String),
            // "Comment", data
            Comment(CommentConst, String),
            // "Character", data
            Character(CharacterConst, String),
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
                        name: name.unwrap_or_default(),
                        public_identifier,
                        system_identifier,
                        force_quirks: !correctness,
                    }),
                    OutputToken::StartTag(_, name, attributes) => Token::StartTag(StartTag {
                        self_closing: false,
                        name,
                        attributes,
                    }),
                    OutputToken::StartTag2(_, name, attributes, self_closing) => {
                        Token::StartTag(StartTag {
                            self_closing,
                            name,
                            attributes,
                        })
                    }
                    OutputToken::EndTag(_, name) => Token::EndTag(EndTag { name }),
                    OutputToken::Comment(_, data) => Token::Comment(data),
                    OutputToken::Character(_, data) => Token::String(data),
                })
                .collect::<Vec<Token>>(),
        ))
    }
}

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Test {
    description: String,
    input: String,
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

#[derive(Debug, Eq, PartialEq)]
struct ParseErrorInner(Error);

impl<'de> Deserialize<'de> for ParseErrorInner {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let str_err = String::deserialize(deserializer)?;
        let err: Error = str_err
            .parse()
            .map_err(|_| D::Error::custom(&format!("failed to deserialize error: {}", str_err)))?;
        Ok(ParseErrorInner(err))
    }
}

#[derive(Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ParseError {
    code: ParseErrorInner,
    // TODO: lineno and column?
}

#[derive(Deserialize)]
struct Tests {
    tests: Vec<Test>,
}

#[test_generator::test_resources("tests/html5lib-tests/tokenizer/*.test")]
fn test_tokenizer_file(resource_name: &str) {
    let path = Path::new(resource_name);
    let fname = path.file_name().unwrap().to_str().unwrap();

    if matches!(
        fname,
        // We don't implement "Coercing an HTML DOM into an infoset" section
        "xmlViolation.test" |
        // Our parser does not operate on bytes, the input isn't valid Rust &str
        "unicodeCharsProblematic.test"
    ) {
        return;
    }

    let f = File::open(path).unwrap();
    let bf = BufReader::new(f);
    let tests: Tests = serde_json::from_reader(bf).unwrap();

    for (i, test) in tests.tests.into_iter().enumerate() {
        run_test(fname, i, test);
    }
}

fn run_test(fname: &str, test_i: usize, mut test: Test) {
    test.input = if test.double_escaped {
        unescape(&test.input)
    } else {
        test.input
    };

    test.output = if test.double_escaped {
        ExpectedOutputTokens(
            test.output
                .0
                .into_iter()
                .map(|token| match token {
                    Token::String(x) => Token::String(unescape(&x)),
                    Token::Comment(x) => Token::Comment(unescape(&x)),
                    token => token,
                })
                .collect(),
        )
    } else {
        ExpectedOutputTokens(test.output.0)
    };

    for state in &test.initial_states {
        run_test_inner(
            fname,
            test_i,
            &test,
            state.0,
            Tokenizer::new(&test.input),
            "string",
        );

        run_test_inner(
            fname,
            test_i,
            &test,
            state.0,
            Tokenizer::new(BufReader::new(test.input.as_bytes())),
            "bufread",
        );
    }
}

fn run_test_inner<R: Reader>(
    fname: &str,
    test_i: usize,
    test: &Test,
    state: State,
    mut tokenizer: Tokenizer<R>,
    tokenizer_info: &str,
) {
    println!(
        "==== FILE {}, TEST {}, STATE {:?}, TOKENIZER {} ====",
        fname, test_i, state, tokenizer_info,
    );
    println!("description: {}", test.description);
    tokenizer.set_internal_state(state);
    tokenizer.set_last_start_tag(test.last_start_tag.as_ref().map(String::as_str));

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

    assert_eq!(test.output.0, actual_tokens);

    if !matches!(
        (fname, test_i),
        // TODO: html5lib-tests bug?
        ("test3.test", 79)
    ) {
        assert_eq!(test.errors, actual_errors);
    }
}

/// Implements the escape sequences described in the tokenizer tests of html5lib-tests (and nothing
/// more)
fn unescape(data: &str) -> String {
    let mut stream = data.chars();
    let mut rv = String::new();

    loop {
        match stream.next() {
            Some('\\') => (),
            Some(x) => {
                rv.push(x);
                continue;
            }
            None => break,
        }

        match stream.next() {
            Some('u') => (),
            x => panic!("unexpected escape: {:?}", x),
        }

        let orig_len = rv.len();

        for _ in 0..4 {
            rv.push(match stream.next() {
                Some(x) => x,
                None => panic!("unexpected eof after \\u"),
            });
        }

        let c = u32::from_str_radix(&rv[orig_len..], 16).expect("failed to parse as hex");
        let c = char::from_u32(c).expect("bad character");
        rv.truncate(orig_len);
        rv.push(c);
    }

    rv
}
