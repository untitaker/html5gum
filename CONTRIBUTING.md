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
