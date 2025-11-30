use std::env;
use std::sync::LazyLock;

mod html5ever;
mod lolhtml;
mod old_html5gum;
mod span_invariants;
mod swc;

// Cache environment variable lookups to avoid repeated syscalls
static FUZZ_BASIC: LazyLock<bool> = LazyLock::new(|| env::var("FUZZ_BASIC").unwrap() == "1");
static FUZZ_OLD_HTML5GUM: LazyLock<bool> =
    LazyLock::new(|| env::var("FUZZ_OLD_HTML5GUM").unwrap() == "1");
static FUZZ_HTML5EVER: LazyLock<bool> =
    LazyLock::new(|| env::var("FUZZ_HTML5EVER").unwrap() == "1");
static FUZZ_LOLHTML: LazyLock<bool> = LazyLock::new(|| env::var("FUZZ_LOLHTML").unwrap() == "1");
static FUZZ_SWC: LazyLock<bool> = LazyLock::new(|| env::var("FUZZ_SWC").unwrap() == "1");
static FUZZ_SPAN_INVARIANTS: LazyLock<bool> =
    LazyLock::new(|| env::var("FUZZ_SPAN_INVARIANTS").unwrap() == "1");

pub fn run(s: &[u8]) {
    let mut did_anything = false;

    if *FUZZ_BASIC {
        // we rely on running in debug mode such that this is not just simply optimized away
        let testing_tokenizer = html5gum::Tokenizer::new(s);
        for Ok(_) in testing_tokenizer {}
        did_anything = true;
    }

    if *FUZZ_OLD_HTML5GUM {
        if let Ok(data) = std::str::from_utf8(s) {
            old_html5gum::run_old_html5gum(data);
        }

        did_anything = true;
    }

    if *FUZZ_HTML5EVER {
        if let Ok(data) = std::str::from_utf8(s) {
            html5ever::run_html5ever(data);
        }

        did_anything = true;
    }

    if *FUZZ_LOLHTML {
        lolhtml::run_lolhtml(s);
        did_anything = true;
    }

    if *FUZZ_SWC {
        if let Ok(data) = std::str::from_utf8(s) {
            swc::run_swc(data);
        }
        did_anything = true;
    }

    if *FUZZ_SPAN_INVARIANTS {
        span_invariants::validate_span_invariants(s);
        did_anything = true;
    }

    if !did_anything {
        panic!("running empty testcase, enable either FUZZ_OLD_HTML5GUM or FUZZ_HTML5EVER");
    }
}
