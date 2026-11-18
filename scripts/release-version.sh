#!/bin/bash
set -xe

[ -z "$(git status --porcelain)" ] || (echo "dirty working directory" && exit 1)

current_version="$(grep '^version = ' Cargo.toml | head -1 | cut -d '"' -f2)"
new_version="$1"

if [ -z "$new_version" ]; then
    echo "New version required as argument"
    exit 1
fi

grep -q "^# $new_version\$" CHANGELOG.md || (echo "CHANGELOG.md has no entry for $new_version" && exit 1)

echo ">>> Bumping version"

sed -i.bak "s/^version = \"$current_version\"/version = \"$new_version\"/" Cargo.toml
rm Cargo.toml.bak

echo ">>> Updating lockfiles"

cargo check
(cd fuzz && cargo check)

echo ">>> Running tests"
cargo build
cargo test

echo ">>> Commit"

git add Cargo.toml fuzz/Cargo.lock
git commit -m "version $new_version"
git tag $new_version

git show HEAD

echo ">>> Publish"

cargo publish
git push origin HEAD
git push origin $new_version
