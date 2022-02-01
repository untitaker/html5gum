# Contributing to html5gum

This project is very small and has little collaboration right now, so feel free
to just open issues if something is unclear or open PRs directly.

What follows is basically a FAQ.

## Testing

html5gum is tested against the [html5lib
testsuite](https://github.com/html5lib/html5lib-tests). This means that for a
successful testrun, you need to initialize git submodules:

```
git submodules update --init
```

_Then_, the tests need to be run with `cargo test --all-features`. `--all-features` is critical, without it, the tests won't compile. Why, you ask?

Because html5gum's testsuite is currently neither integration test nor unit test (see [the Rust book for the Rust-specific definitions of what those are](https://doc.rust-lang.org/book/ch11-03-test-organization.html)).

* It is suboptimal to have as unit test, because the folder of `n` JSON files we get from the html5lib-tests repo should map to `n * m` tests (each file contains multiple testcases). There are two ways to achieve that:

    * Procedural macro that generates `#[test] fn ..` code. We used to actually use the `test_generator` crate to achieve that, but it produces only one testcase per file.

    * [Custom test harness](https://rust-lang.github.io/rfcs/2318-custom-test-frameworks.html), which is what we have now, and which doesn't work at all for unit tests.

* It is _also_ suboptimal as integration test, as the testsuite needs access to some private APIs of html5gum.

Therefore a hack was born: We use integration tests, and additionally add a featureflag to expose private APIs to `html5gum`. That featureflag is called `integration-tests`.

## No unsafe Rust? No dependencies?

`html5gum` does not contain unsafe Rust. Right now there's no real necessity for this, but my personal opinion is that I am too dumb to get unsafe code right. And the stakes are very high for something that is supposed to be able to parse untrusted HTML! While the rules around unsafe code are still changing, it's too risky as well.

`html5gum` also currently does not have dependencies. This might change over time, but right now I just got by without it.

This is really the main reason for both the unsafe and dependency policy: I just got by without it so far.
