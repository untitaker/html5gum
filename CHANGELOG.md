# Unreleased

- Change types around `Token::Doctype` so that `.value` is no longer necessary. [PR 139](https://github.com/untitaker/html5gum/pull/139)
- Stop formatting internal trace messages while trace logging is off. This made builds with debug assertions, such as the dev profile of downstream crates, slow and allocate in proportion to the input. [PR 147](https://github.com/untitaker/html5gum/pull/147)
- Fix `CallbackEmitter` reporting unquoted attribute value spans that start one byte late. [PR 146](https://github.com/untitaker/html5gum/pull/146)
- Fix spans of text that ends at EOF in the RCDATA, RAWTEXT, script data and character reference states, which ended one byte early. [PR 148](https://github.com/untitaker/html5gum/pull/148)
- Speed up named character references such as `&amp;` by only trying names that start with the same two characters. [PR 150](https://github.com/untitaker/html5gum/pull/150)
- Pass reconsumed characters to the next state by value instead of through the reader. This simplifies the reader and lets some states handle the reconsumed character without a round-trip through the state machine.
- Search for delimiters with a portable SWAR implementation, which processes 8 bytes at a time, and make `jetscii` an optional feature that is disabled by default. The new search is faster than jetscii except when SSE4.2 is enabled at compile time.

# 0.8.4

- Fix an ordering bug when using `CallbackEmitter`. [PR 135](https://github.com/untitaker/html5gum/pull/135)

# 0.8.3

- Fix more bugs in span position tracking. [PR 131](https://github.com/untitaker/html5gum/pull/131)

# 0.8.2

- Make upgrading to 0.8.0 smoother by adding more `From`-impls. [PR 126](https://github.com/untitaker/html5gum/pull/126)
- Fix many more bugs in span position tracking. [PR 130](https://github.com/untitaker/html5gum/pull/130)

# 0.8.1

- Fix a bug in spans position tracking. [PR 124](https://github.com/untitaker/html5gum/pull/124)

# 0.8.0

- Experimental support for spans, i.e. reporting the locations of errors and tokens in the original source. [PR 120](https://github.com/untitaker/html5gum/pull/120)
- There are many breaking changes coming from this, but we did our best to make
  most of them less painful.

# 0.7.0

- Removal of `Tokenizer.infallible()`. Use `for Ok(token) in Tokenizer::new()` instead. [PR 102](https://github.com/untitaker/html5gum/pull/102)
- Add more convenience functions to `tree-builder` feature, equivalent to `html5ever::driver`. [PR 101](https://github.com/untitaker/html5gum/pull/101)

# 0.6.1

- Fix a bug where html5gum would interpret tags inside of `<script>`. [PR 98](https://github.com/untitaker/html5gum/pull/98)
- Restructured the crate slightly, though there _should_ not be any breaking changes. [PR 99](https://github.com/untitaker/html5gum/pull/99)
- Added a way to integrate with `scraper` crate and the `html5ever` tree builder, see `examples/scraper.rs`.

# Before 0.6.1

Who knows...
