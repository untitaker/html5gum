clean:
	rm -rf target target-afl in out

in:
	mkdir -p in
	curl https://www.nytimes.com/ > in/nytimes.html
	curl https://docs.sentry.io/ > in/sentrydocs.html

setup-afl: in
	which cargo-afl || cargo install afl
	CARGO_TARGET_DIR=./target-afl/ cargo afl build --bin html5gum-fuzz-afl

afl: setup-afl
	CARGO_TARGET_DIR=./target-afl/ AFL_AUTORESUME=1 cargo afl fuzz -i in -o out target-afl/debug/html5gum-fuzz-afl

try-afl-crashes:
	set -e && for f in out/default/crashes/id:*; do \
		echo $$f; \
		$(MAKE) cli < $$f; \
	done

cli:
	cargo run --bin html5gum-fuzz-cli
