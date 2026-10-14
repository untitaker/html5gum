# Contributing to html5gum

## AI policy

PR descriptions and issues should be human-authored. There is no ban on
AI-generated code in this repo.

This policy is mainly there so that people properly explain their motivations
and keep their PR descriptions terse.

## Testing

html5gum is tested against the [html5lib
testsuite](https://github.com/html5lib/html5lib-tests). This means that for a
successful testrun, you need to initialize git submodules:

```
git submodules update --init
```

Then, `cargo test` should just work.
