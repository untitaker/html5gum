# Unreleased

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
