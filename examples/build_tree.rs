/// Using the `tree-builder` feature, plug html5gum's tokenizer into html5ever's tree
/// building logic and DOM implementation. The result is a technically complete HTML5 parser.
///
/// You may want to refer to `examples/scraper.rs` for better ergonomics.

use html5ever::tree_builder::TreeBuilder;
use html5gum::{Html5everEmitter, IoReader, Tokenizer};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

fn walk(indent: usize, handle: &Handle) {
    let node = handle;
    // FIXME: don't allocate
    print!("{}", " ".repeat(indent));
    match node.data {
        NodeData::Document => println!("#Document"),

        NodeData::Doctype {
            ref name,
            ref public_id,
            ref system_id,
        } => println!("<!DOCTYPE {} \"{}\" \"{}\">", name, public_id, system_id),

        NodeData::Text { ref contents } => {
            println!("#text: {}", contents.borrow().escape_default())
        }

        NodeData::Comment { ref contents } => println!("<!-- {} -->", contents.escape_default()),

        NodeData::Element {
            ref name,
            ref attrs,
            ..
        } => {
            print!("<{}", name.local);
            for attr in attrs.borrow().iter() {
                print!(" {}=\"{}\"", attr.name.local, attr.value);
            }
            println!(">");
        }

        NodeData::ProcessingInstruction { .. } => unreachable!(),
    }

    for child in node.children.borrow().iter() {
        walk(indent + 4, child);
    }
}

fn main() {
    let rcdom = RcDom::default();
    let mut tree_builder = TreeBuilder::new(rcdom, Default::default());
    let token_emitter = Html5everEmitter::new(&mut tree_builder);

    let tokenizer =
        Tokenizer::new_with_emitter(IoReader::new(std::io::stdin().lock()), token_emitter);

    tokenizer.finish().unwrap();

    let rcdom = tree_builder.sink;

    walk(0, &rcdom.document);

    let errors = rcdom.errors;

    if !errors.is_empty() {
        println!("\nParse errors:");

        for err in errors.iter() {
            println!("    {}", err);
        }
    }
}
