/// Shows how to use html5gum in combination with scraper.
///
/// Usage:
///
/// ```sh
/// echo '<h1><span class=hello>Hello</span></h1>' | cargo run --all-features --example scraper
/// ```
///
/// Essentially, your HTML parsing will be powered by a combination of html5gum and html5ever. This
/// has no immediate benefit over using scraper normally and is mostly done as a transitionary step
/// until html5gum has its own implementation of tree building and the DOM.
///
/// Requires the tree-builder feature.
use std::io::{stdin, Read};

use argh::FromArgs;
use html5gum::emitters::html5ever::parse_document;
use html5ever::interface::tree_builder::TreeSink;
use scraper::{Html, HtmlTreeSink, Selector};

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

    let mut input = String::new();
    stdin().read_to_string(&mut input).unwrap();

    let dom = if cli.use_html5ever {
        Html::parse_document(&input)
    } else {
        let dom = Html::new_document();
        let tree_sink = HtmlTreeSink::new(dom);
        let Ok(tree_sink) = parse_document(&input, tree_sink, Default::default());
        tree_sink.finish()
    };

    let selector = Selector::parse(&cli.selector).unwrap();

    for element in dom.select(&selector) {
        println!("{:?}", element);
    }
}
