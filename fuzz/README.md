# fuzzing html5gum

html5gum is fuzzed using both [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) and [afl.rs](https://github.com/rust-fuzz/afl.rs). AFL is the more mature setup of the two.

## Fuzz target

Both fuzzers run against the same test target defined in `src/testcase.rs`.
That testcase can be configured using a variety of environment variables. See
the beginning of `Makefile`. Absence of any of those envvars will crash the
target.

* `FUZZ_BASIC=1` to run html5gum on the input, exhaust the token iterator but
  discard the output. This can only find crashes and hangs.
* `FUZZ_OLD_HTML5GUM=1` to run html5gum against an older version of itself, and
  crash when html5gum produces different output than the old "reference
  version". This can be used to find bugs in patches to html5gum.

  `FUZZ_IGNORE_PARSE_ERRORS` is an envvar that can be used in this context to
  post-process parsing errors. This was added because sometimes the "reference
  version" does have bugs, and we'd like to skip over those bugs.

  * `FUZZ_IGNORE_PARSE_ERRORS=order` will sort errors from both parsers such that order can be ignored.
  * `FUZZ_IGNORE_PARSE_ERRORS=1` will delete all errors so that parsing errors are not compared at all.
  * `FUZZ_IGNORE_PARSE_ERRORS=if-reference-contains:duplicate-attribute` will delete all errors _if_ any of them _in the old version of html5gum_ contains the string `duplicate-attribute`.
  * `FUZZ_IGNORE_PARSE_ERRORS=if-testing-contains:duplicate-attribute` will delete all errors _if_ any of them _in the new version of html5gum_ contains the string `duplicate-attribute`.

  This envvar is a comma-separated list of instructions. For example,
  `FUZZ_IGNORE_PARSE_ERRORS=order,if-reference-contains:foo` means "ignore
  order, but also ignore errors entirely if old html5gum emitted an error
  containing `foo`".

  By default this variable is empty, meaning errors are compared exactly.

* `FUZZ_HTML5EVER=1` to run html5gum and html5ever, and crash when the produced

  tokens are different. Note that [html5ever currently does not pass the
  html5lib testsuite and lags behind on
  spec](https://github.com/servo/html5ever/issues/459).

* `FUZZ_LOLHTML=1` to run html5gum and lol-html, and crash when the produced
  tokens are different.

## Basic CLI

Run `FUZZ_BASIC=1 make -e cli` to run the fuzz target itself as a barebones CLI
tool, taking input on stdin and printing debug representation of tokens to
stdout.

## AFL

* Run `FUZZ_BASIC=1 make -e afl` to run a basic fuzz that will only find crashes.
* Run `FUZZ_BASIC=1 _AFL_OPTS='-S fuzzer02' make -e afl` to run another "slave"
  fuzzer, such that multiple cores can be used effectively. The master fuzzer
  is called `fuzzer01`.
* Run `FUZZ_BASIC=1 make -e afl-next` after fuzzing to get the next crash and
  run afl-tmin on it. It will print the testcase as JSON string to check back
  into e.g. a file in `tests/custom-html5lib-tests/`.

## cargo fuzz

* Run `FUZZ_BASIC=1 make -e libfuzzer` to run a basic fuzz much like with afl.
* Use `_LIBFUZZER_OPTS='--jobs 8'` to use more cores.
* Run `FUZZ_BASIC=1 make -e sh` to drop in a shell where all envvars for the
  fuzz target are set. Then use the subcommands by `cargo-fuzz` to go through
  testcases.

**By default no sanitizer is run.** This probably should be fixed, but the default
asan causes too many false-positives.

## Corpus / in-dir

`in/` contains test files that are used to seed the fuzzer. The makefile will
read from the html5lib-tests submodule to populate that directory.

You can use `make in` to initialize it explicitly, then modify the contents
yourself.

In the case of `cargo-fuzz`, the directory will be populated with additional
testcases found, in the case of AFL, it will not be touched during fuzzing.

AFL and `cargo-fuzz` can be run in alternating ways, and will pick up each
other's testcases to some degree, as `cargo-fuzz` is wired to read from both
`in/` and `out/`, and AFL is wired to read from `in/` and manage its queue in
`out/`. See [AFL compatibility in libfuzzer
docs](https://llvm.org/docs/LibFuzzer.html#afl-compatibility).
