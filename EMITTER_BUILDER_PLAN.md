# Plan: replace `CallbackEmitter` with `EmitterBuilder`

## Motivation

`CallbackEmitter` routes all events through one closure that matches on a
`CallbackEvent` enum. The closure inlines fine, but which events the user cares
about is a runtime property, so the adapter must maintain *every* buffer
(comments, doctype, coalesced text) unconditionally. LLVM cannot delete
`Vec::extend` into a long-lived struct field just because the only reader is a
dead match arm. Additionally, `CallbackEmitter` never overrides
`should_emit_errors`, so the whole error-detection machinery in
`char_validator` stays live even for callbacks that ignore errors.

The fix: encode *which handlers exist* in the type, so buffer maintenance can
be gated on constants that fold at monomorphization. Same trick as the
existing `SpanBound` / `Span<()>` mechanism, generalized to all event
categories.

A second goal is fixing a usability wart: the flat event stream forces users
to carry state between events (e.g. `is_in_span` in the docs example) to know
which tag an attribute belongs to — even though the emitter already buffers
the tag name for `current_is_appropriate_end_tag_token`. The attribute handler
should receive the tag name directly.

## Design

One concrete type, `EmitterBuilder` (module `src/emitters/builder.rs`),
implementing `Emitter` directly. Each handler slot is a type parameter; an
empty slot is `()`, a filled slot is `Handler<F>` (private newtype around the
user's closure). A **private, sealed** `Slot` trait provides:

```rust
trait Slot<St, Args> {
    const PRESENT: bool;
    fn call(&mut self, state: &mut St, args: Args);
}
// impl for (): PRESENT = false, call is a no-op
// impl for Handler<F> where F: FnMut(&mut St, Args): PRESENT = true
```

The newtype exists because coherence forbids `impl Slot for ()` next to a
blanket `impl<F: FnMut> Slot for F` (rustc won't use "`()` is not a closure"
as a disjointness proof). Users never see it; `on_*` methods wrap internally.

Decisions already made in prior discussion, recorded here:

- **Sealed trait, not const-generics-only.** A pure const-generic variant
  (one `const X: bool` per slot plus fn-pointer defaults) works but roughly
  doubles the type parameter count and produces worse error messages. Both
  compile to identical code. The trait stays private/sealed, so the public
  surface is still "one concrete type plus builder methods".
- **Presence cannot go out of sync with the handler.** Registering a closure
  is the only way to flip the type-level bit; there is no user-visible flag to
  forget (this replaces the `should_emit_errors` footgun class entirely).
- **All handlers return `()`.** Token emission (needed by `DefaultEmitter` and
  the `Tokenizer` iterator) goes through a dedicated `on_pop_token` slot
  instead of `Option<T>` returns everywhere; see below. This keeps closure
  bodies free of trailing `None`s.
- **Rejected alternatives**, so nobody re-derives them: a public visitor
  trait with `const WANTS_X: bool` demand flags (flags can silently go out
  of sync with overridden methods — same footgun class as forgetting to
  override `should_emit_errors` today); a macro that generates the impl and
  derives the flags from which methods are written (works, but macro
  ergonomics and users can bypass it); a pull/lending API
  (`while let Some(event) = t.next_event()`) — nicest control flow but
  doesn't change the dead-code story (demand must still be declared
  statically) and needs event staging inside the tokenizer; could be built
  on top of the builder later.

### Public API sketch

```rust
use html5gum::emitters::builder::sink;

let emitter = sink(MyState::default())          // St = MyState, S = ()
    .on_attribute(|st, tag: &[u8], name: &[u8], value: &[u8], spans| { ... })
    .on_text(|st, text: &[u8], span| { ... });

// with spans (S chosen at construction; it is baked into handler
// signatures, so it cannot change after the first handler is registered):
let emitter = spanned_sink::<usize, _>(MyState::default())...
```

Handler slots (all optional, positional args plus trailing span info):

| builder method | args (after `&mut St`) | notes |
|---|---|---|
| `on_tag_open` | `name, Span<S>` | start tag name known, attributes not yet read |
| `on_attribute` | `tag_name, attr_name, attr_value, AttrSpans<S>` | name+value delivered together; `AttrSpans` carries name span and value span (both needed by `DefaultEmitter`) |
| `on_tag_close` | `tag_name, self_closing, Span<S>` | end of start tag |
| `on_end_tag` | `name, Span<S>` | |
| `on_text` | `text, Span<S>` | coalesced, same guarantee as `CallbackEvent::String` today |
| `on_comment` | `value, Span<S>` | |
| `on_doctype` | `Doctype<'_>, Span<S>` | name, public/system identifier, force_quirks |
| `on_error` | `Error, Span<S>` | presence of this slot **is** `should_emit_errors()` |
| `on_pop_token` | `-> Option<T>` | sets `Emitter::Token = T`; absent slot means `Token = Infallible`, `pop_token` returns `None` |

Runtime options kept as plain setters (cheap, no type-level need):
`naively_switch_states(bool)`.

Also needed: `state()` / `state_mut()` accessors for `St`. Handlers take
`&mut St` as their first argument because separate closures cannot jointly
borrow shared mutable state — the builder owns the state and threads it in.
The accessors are the outside world's door to that state; the html5ever
bridge depends on this (it reads `next_state` out of its state after
`emit_current_tag`, today via `callback_mut()`, see html5ever.rs:184), and
`on_pop_token`-style consumers may want it after tokenizing.

### Gating rules (the actual optimization)

Every `Emitter` method body in the builder is gated on slot presence:

- attribute name/value buffers: written only if `OnAttribute::PRESENT`
- comment buffer: only if `OnComment::PRESENT`
- text coalescing buffer: only if `OnText::PRESENT`
- doctype buffers: only if `OnDoctype::PRESENT`
- `should_emit_errors()` returns `OnError::PRESENT` (`#[inline]`)
- span/position tracking: already free via `S = ()`, unchanged

Always-on regardless of slots (required for correct tokenization):
`current_tag_name`, `last_start_tag`, `current_is_appropriate_end_tag_token`,
current tag type (start/end). This matches what a correct hand-written
`Emitter` (e.g. hyperlink's) must maintain anyway, so for any given demand
set the builder should compile to essentially the hand-written equivalent.

**Deliberate behavior change vs `CallbackEmitter`:** errors are off unless
`on_error` is registered. `CallbackEmitter` always ran error detection.
Document this in the changelog and module docs.

## Implementation steps

1. **`src/emitters/builder.rs`**: `EmitterBuilder`, sealed `Slot` trait,
   `Handler` newtype, `sink()` / `spanned_sink()` constructors, `Emitter`
   impl with gating as above. Port the `round_trip` test from
   `callback.rs:626` and the span assertions. Add a test that an
   empty-slot builder still tokenizes correctly (appropriate-end-tag
   behavior, `<script>`/rcdata handling with `naively_switch_states`).
2. **Port `DefaultEmitter`** (`src/emitters/default.rs`): `St` becomes the
   current `OurCallback` state (tag name, `BTreeMap` attribute map with
   duplicate detection, span bookkeeping) plus a `VecDeque<Token<S>>`.
   All slots registered including `on_error` and `on_pop_token`. Public API
   (`Token`, `StartTag`, `EndTag`, `Doctype`, `new_with_span`,
   `naively_switch_states`) unchanged. The html5lib-tests suite
   (`tests/html5lib_tokenizer.rs`) and `tests/test_spans.rs` must pass
   unchanged — they are the conformance gate.
3. **Port the html5ever bridge** (`src/emitters/html5ever.rs`): its
   `OurCallback` becomes the builder `St`. It keeps its existing
   `ForwardingEmitter` wrapper that overrides `emit_current_tag` to take
   `next_state` out of the state (html5ever.rs:182) — no new builder API
   needed for state switching.
4. **Port examples and docs**: `examples/callback_emitter.rs`,
   `examples/spans.rs`, the doc example in `src/tokenizer.rs:133`, the
   module docs in `src/emitters/mod.rs:16` and `callback.rs` module docs
   (moved to `builder.rs`).
5. **Remove `CallbackEmitter`** (`src/emitters/callback.rs`, the `Callback`
   trait, `CallbackEvent`). Recommendation: outright removal in 0.9.0 rather
   than a deprecation cycle — the crate is pre-1.0, and a shim would keep the
   always-buffering path alive. Changelog gets a migration table:
   `CallbackEvent::OpenStartTag` → `on_tag_open`, `AttributeName`/
   `AttributeValue` → `on_attribute` (now paired, with tag name),
   `CloseStartTag` → `on_tag_close`, `EndTag` → `on_end_tag`, `String` →
   `on_text`, `Comment` → `on_comment`, `Doctype` → `on_doctype`,
   `Error(e)` → `on_error`, "return `Some(token)`" → `on_pop_token` +
   queue in state. Note the errors-now-off-by-default change.
6. **Changelog + version bump to 0.9.0.**

## Benchmarks

Run everything on the same machine, same rustc (record `rustc -V`), before
(current `main`, d182efa) and after. Use hyperfine.

Compile time — html5gum itself:

```sh
hyperfine --warmup 1 \
  --prepare 'cargo clean' 'cargo build' \
  --prepare 'cargo clean' 'cargo build --release' \
  --prepare 'cargo clean' 'cargo check'
# incremental:
hyperfine --warmup 2 --prepare 'touch src/lib.rs' 'cargo build'
```

Compile time — hyperlink (downstream consumer; uses the raw `Emitter` trait,
so this is a control — expect ~no change). Patch its html5gum dependency to a
local path for the "after" run:

```sh
cd ../hyperlink
hyperfine --warmup 1 --prepare 'cargo clean' 'cargo build --release'
hyperfine --warmup 2 --prepare 'touch src/main.rs' 'cargo build --release'
```

Monomorphization volume (the mechanism behind compile-time changes):
`cargo llvm-lines` on the crate's test/bench targets before/after, compare
top ~30 entries. If compile time regresses, this shows where.

Runtime (DefaultEmitter now routes through the builder, must be neutral):

```sh
cargo bench   # benches/patterns.rs, benches/files.rs
```

Codegen spot check (optional but cheap): compare `cargo asm` /
`--emit=llvm-ir` output of a builder with only `on_attribute` registered
against a hand-written no-op-heavy `Emitter`, confirming the comment/doctype
paths compile to empty functions.

## Acceptance criteria

- html5lib-tests and `test_spans` pass unchanged.
- `cargo bench` within noise of `main`.
- Cold and incremental build times for html5gum and hyperlink within ~5% of
  `main`; if worse, investigate with `cargo llvm-lines` before merging.
- All examples compile and produce the same output.

## Risks / open questions

- **Compile time** is the main open risk (hence the benchmarks): the builder
  monomorphizes per handler combination, but `CallbackEmitter` already
  monomorphized per closure type, so the expectation is roughly neutral.
- **Type-error verbosity**: ~10 type parameters on `EmitterBuilder`. Sealed
  trait keeps it as small as this design allows; errors will show
  `Handler<{closure}>` / `()` per slot. Acceptable, but check what common
  mistakes (wrong closure signature) look like and add `#[doc]` guidance.
- **Unnameable emitter types**: closures make the built emitter type
  unnameable, so it can't be stored in a struct field without generics. The
  raw `Emitter` trait remains the escape hatch for that (hyperlink-style
  consumers are unaffected either way).
- **Bikeshed**: positional handler args vs. per-event structs
  (`on_attribute(|st, ev: AttributeEvent<'_, S>| ...)`). Structs are more
  extensible (adding a field isn't a breaking change to every closure);
  positional is less noise. Plan assumes positional with a spans struct only
  where two spans are needed; flip to event structs if extensibility wins.
- **Follow-up, out of scope**: streaming (non-coalesced) text as an opt-in
  slot variant to skip the text copy entirely; porting hyperlink onto the
  builder as an ergonomics validation.
