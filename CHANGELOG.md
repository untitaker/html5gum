# 0.9.0

- Replaced `CallbackEmitter` with `EmitterBuilder` (`html5gum::emitters::builder`). Instead of
  routing every event through one closure matching on a `CallbackEvent` enum, each kind of event
  gets its own optional handler slot (`on_tag_open`, `on_attribute`, ...), registered via builder
  methods. Only the buffers a registered handler actually needs are maintained; unregistered
  handlers cost nothing at runtime.
- **Breaking:** parsing errors are no longer computed or delivered by default. `CallbackEmitter`
  always ran error detection; with `EmitterBuilder`, errors are off unless you register
  `on_error`.
- **Breaking:** attribute name and value are now delivered together in one `on_attribute` call
  (with the tag name included) instead of as two separate `AttributeName`/`AttributeValue`
  events. This also means user closures can no longer capture shared mutable state from their
  environment (since each event kind is a separate closure); shared state must now be passed
  through the emitter's `St` type parameter and threaded via `&mut St`, the handler's first
  argument.
- `EmitterBuilder` also offers `on_text_chunk` as a streaming alternative to `on_text`: chunks
  are delivered as the tokenizer produces them, with no coalescing buffer (and no copy of text
  content), at the cost of arbitrary chunk boundaries. The two fill the same handler slot, so
  only one of them can be registered.
- Migration table, `CallbackEvent` variant -> `EmitterBuilder` method:
  - `OpenStartTag` -> `on_tag_open`
  - `AttributeName` + `AttributeValue` -> `on_attribute` (now paired, with tag name)
  - `CloseStartTag` -> `on_tag_close` (now also receives the tag name)
  - `EndTag` -> `on_end_tag`
  - `String` -> `on_text`
  - `Comment` -> `on_comment`
  - `Doctype` -> `on_doctype`
  - `Error(e)` -> `on_error`
  - returning `Some(token)` from the callback -> `on_pop_token`, with the token queued in `St`

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
