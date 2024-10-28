/// Shows how to use html5gum in combination with scraper.
///
/// Usage:
///
/// ```sh
/// echo '<h1><span class=hello>Hello</span></h1>' | cargo run --all-features --example scraper
/// ```
///
/// Essentially, your HTML parsing will be powered by a combination of html5gum and html5ever.
///
/// Requires the tree-builder feature.
use std::io::{stdin, Read};

use html5ever::tree_builder::TreeBuilder;
use html5gum::{Html5everEmitter, IoReader, Tokenizer};
use scraper::{Html, Selector};

use argh::FromArgs;

/// Read some HTML from stdin and parse it according to the given selector.
#[derive(FromArgs)]
struct Cli {
    /// turn off html5gum and just use regular scraper.
    ///
    /// This can be useful for comparing the two in performance and correctness.
    #[argh(switch)]
    use_html5ever: bool,

    /// a CSS selector, like ".hello"
    #[argh(positional)]
    selector: String,
}

fn main() {
    let cli: Cli = argh::from_env();

    let dom = if cli.use_html5ever {
        let mut input = String::new();
        stdin().read_to_string(&mut input).unwrap();
        Html::parse_document(&input)
    } else {
        // parsing the document
        let dom = Html::new_document();
        let mut tree_builder = TreeBuilder::new(dom, Default::default());
        let token_emitter = Html5everEmitter::new(&mut tree_builder);
        let reader = IoReader::new(stdin().lock());
        let tokenizer = Tokenizer::new_with_emitter(reader, token_emitter);

        tokenizer.finish().unwrap();
        tree_builder.sink
    };

    let selector = Selector::parse(&cli.selector).unwrap();

    for element in dom.select(&selector) {
        println!("{:?}", element);
    }
}
