// A lot of this test harness has been copied from html5ever.
//
// Copyright 2014-2017 The html5ever Project Developers. See the
// COPYRIGHT file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
use std::{
    fs::File,
    io::{BufRead, BufReader},
    iter::repeat,
    path::Path,
};

use glob::glob;
use libtest_mimic::{self, Arguments, Trial};

use html5ever::tree_builder::{TreeBuilder, TreeBuilderOpts};
use html5ever::{namespace_url, ns};
use html5gum::{testutils::trace_log, Html5everEmitter, Tokenizer};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use pretty_assertions::assert_eq;

mod testutils;

#[derive(Default, Debug, Clone)]
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
            // TODO
            continue;
        }

        // if script_on is not explicitly provided, it's ok to run this test with scripting
        // disabled
        if testcase.script_on.is_none() {
            tests.push(build_test(testcase.clone(), fname, i, false));
        }

        // if script_off is not explicitly provided, it's ok to run this test with scripting
        // enabled
        if testcase.script_off.is_none() {
            tests.push(build_test(testcase, fname, i, true));
        }
    }
}

fn build_test(testcase: Testcase, fname: &str, i: usize, scripting: bool) -> Trial {
    let scripting_text = if scripting { "yesscript" } else { "noscript" };
    Trial::test(format!("{}:{}:{scripting_text}", fname, i), move || {
        testutils::catch_unwind_and_report(move || {
            trace_log(&format!("{:#?}", testcase));
            let rcdom = RcDom::default();
            let mut opts = TreeBuilderOpts::default();
            opts.scripting_enabled = scripting;

            let mut tree_builder = TreeBuilder::new(rcdom, opts);
            let token_emitter = Html5everEmitter::new(&mut tree_builder);

            let input = testcase.data.trim_end_matches('\n');

            let tokenizer = Tokenizer::new_with_emitter(input, token_emitter);

            for result in tokenizer {
                result.unwrap();
            }

            let rcdom = tree_builder.sink;
            let mut stringified_result = String::new();
            for child in rcdom.document.children.borrow().iter() {
                serialize(&mut stringified_result, 1, child.clone());
            }

            assert_eq!(stringified_result, testcase.document.unwrap());
        })
    })
}

fn serialize(buf: &mut String, indent: usize, handle: Handle) {
    buf.push_str("|");
    buf.push_str(&repeat(" ").take(indent).collect::<String>());

    let node = handle;
    match node.data {
        NodeData::Document => panic!("should not reach Document"),

        NodeData::Doctype {
            ref name,
            ref public_id,
            ref system_id,
        } => {
            buf.push_str("<!DOCTYPE ");
            buf.push_str(&name);
            if !public_id.is_empty() || !system_id.is_empty() {
                buf.push_str(&format!(" \"{}\" \"{}\"", public_id, system_id));
            }
            buf.push_str(">\n");
        }

        NodeData::Text { ref contents } => {
            buf.push_str("\"");
            buf.push_str(&contents.borrow());
            buf.push_str("\"\n");
        }

        NodeData::Comment { ref contents } => {
            buf.push_str("<!-- ");
            buf.push_str(&contents);
            buf.push_str(" -->\n");
        }

        NodeData::Element {
            ref name,
            ref attrs,
            ..
        } => {
            buf.push_str("<");
            match name.ns {
                ns!(svg) => buf.push_str("svg "),
                ns!(mathml) => buf.push_str("math "),
                _ => (),
            }
            buf.push_str(&*name.local);
            buf.push_str(">\n");

            let mut attrs = attrs.borrow().clone();
            attrs.sort_by(|x, y| x.name.local.cmp(&y.name.local));
            // FIXME: sort by UTF-16 code unit

            for attr in attrs.into_iter() {
                buf.push_str("|");
                buf.push_str(&repeat(" ").take(indent + 2).collect::<String>());
                match attr.name.ns {
                    ns!(xlink) => buf.push_str("xlink "),
                    ns!(xml) => buf.push_str("xml "),
                    ns!(xmlns) => buf.push_str("xmlns "),
                    _ => (),
                }
                buf.push_str(&format!("{}=\"{}\"\n", attr.name.local, attr.value));
            }
        }

        NodeData::ProcessingInstruction { .. } => unreachable!(),
    }

    for child in node.children.borrow().iter() {
        serialize(buf, indent + 2, child.clone());
    }

    if let NodeData::Element {
        ref template_contents,
        ..
    } = node.data
    {
        if let Some(ref content) = &*template_contents.borrow() {
            buf.push_str("|");
            buf.push_str(&repeat(" ").take(indent + 2).collect::<String>());
            buf.push_str("content\n");
            for child in content.children.borrow().iter() {
                serialize(buf, indent + 4, child.clone());
            }
        }
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
