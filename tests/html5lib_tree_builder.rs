use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use glob::glob;
use libtest_mimic::{self, Arguments, Trial};

use html5ever::tree_builder::TreeBuilder;
use html5gum::{testutils::trace_log, Html5everEmitter, Tokenizer};
use markup5ever_rcdom::RcDom;

mod testutils;

#[derive(Default, Debug)]
struct Testcase {
    data: String,
    errors: Option<String>,
    new_errors: Option<String>,
    document_fragment: Option<String>,
    script_off: Option<String>,
    script_on: Option<String>,
    document: Option<String>,
}

impl Testcase {
    fn parse(path: &Path, iter: impl Iterator<Item = std::io::Result<Vec<u8>>>) -> Option<Self> {
        let mut rv = Testcase::default();
        let mut current_field: Option<&mut String> = Some(&mut rv.data);
        let mut has_errors = false;

        for line in iter {
            let line = String::from_utf8(line.unwrap()).unwrap();

            match line.as_str() {
                "#data" => {
                    if let Some(ref mut field) = current_field {
                        if field.ends_with("\n\n") {
                            field.pop();
                        }

                        if has_errors {
                            return Some(rv);
                        }
                    }
                }
                "#errors" => {
                    current_field = Some(rv.errors.get_or_insert_with(Default::default));
                    has_errors = true;
                }
                "#new-errors" => {
                    current_field = Some(rv.new_errors.get_or_insert_with(Default::default))
                }
                "#document-fragment" => {
                    current_field = Some(rv.document_fragment.get_or_insert_with(Default::default))
                }
                "#script-on" => {
                    current_field = Some(rv.script_on.get_or_insert_with(Default::default))
                }
                "#script-off" => {
                    current_field = Some(rv.script_off.get_or_insert_with(Default::default))
                }
                "#document" => {
                    current_field = Some(rv.document.get_or_insert_with(Default::default))
                }
                x => match current_field {
                    Some(ref mut current_field) => {
                        current_field.push_str(x);
                        current_field.push('\n');
                    }
                    None => {
                        panic!("{:?}: Unexpected character: {:?}", path, x);
                    }
                },
            }
        }

        None
    }
}

fn produce_testcases_from_file(tests: &mut Vec<Trial>, path: &Path) {
    let fname = path.file_name().unwrap().to_str().unwrap();

    let mut lines_iter = BufReader::new(File::open(path).unwrap())
        .split(b'\n')
        .peekable();

    let mut i = 0;

    while let Some(testcase) = Testcase::parse(path, &mut lines_iter) {
        i += 1;

        if testcase.document_fragment.is_some() {
            continue;
        }

        if testcase.script_on.is_some() {
            continue;
        }

        tests.push(Trial::test(format!("{:?}:{}", fname, i), move || {
            testutils::catch_unwind_and_report(move || {
                trace_log(&format!("{:#?}", testcase));
                let rcdom = RcDom::default();
                let mut tree_builder = TreeBuilder::new(rcdom, Default::default());
                let token_emitter = Html5everEmitter::new(&mut tree_builder);

                let input = testcase.data.trim_end_matches('\n');

                let tokenizer = Tokenizer::new_with_emitter(input, token_emitter);

                for result in tokenizer {
                    result.unwrap();
                }

                let _rcdom = tree_builder.sink;
            })
        }));
    }
}

fn main() {
    let args = Arguments::from_args();
    let mut tests = Vec::new();

    for entry in glob("tests/html5lib-tests/tree-construction/*.dat").unwrap() {
        produce_testcases_from_file(&mut tests, &entry.unwrap());
    }

    libtest_mimic::run(&args, tests).exit();
}
