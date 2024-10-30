# 0.7.0

- Removal of `Tokenizer.infallible()`. Use `for Ok(token) in Tokenizer::new()` instead. [PR 102](https://github.com/untitaker/html5gum/pull/102)
- Add more convenience functions to `tree-builder` feature, equivalent to `html5ever::driver`. [PR 101](https://github.com/untitaker/html5gum/pull/101)

# 0.6.1

- Fix a bug where html5gum would interpret tags inside of `<script>`. [PR 98](https://github.com/untitaker/html5gum/pull/98)
- Restructured the crate slightly, though there _should_ not be any breaking changes. [PR 99](https://github.com/untitaker/html5gum/pull/99)
- Added a way to integrate with `scraper` crate and the `html5ever` tree builder, see `examples/scraper.rs`.

# Before 0.6.1

Who knows...
