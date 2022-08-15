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

Then, `cargo test` should just work.

## No unsafe Rust? No dependencies?

`html5gum` does not contain unsafe Rust. Right now there's no real necessity for this, but my personal opinion is that I am too dumb to get unsafe code right. And the stakes are very high for something that is supposed to be able to parse untrusted HTML! While the rules around unsafe code are still changing, it's too risky as well.

`html5gum` also currently does not have dependencies. This might change over time, but right now I just got by without it.

This is really the main reason for both the unsafe and dependency policy: I just got by without it so far.
