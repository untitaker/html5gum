AFL_TARGET_BIN=target-afl/debug/html5gum-fuzz-afl

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
	CARGO_TARGET_DIR=./target-afl/ AFL_AUTORESUME=1 cargo afl fuzz $$_AFL_OPTS -i in -o out ${AFL_TARGET_BIN}

afl-tmin:
	set -e && for f in out/*/crashes/id:*; do \
		echo $$f; \
		if ! $(MAKE) cli < $$f; then \
			afl-tmin -i $$f -o /tmp/html5gum-mintest ${AFL_TARGET_BIN}; \
			exit 2; \
		fi \
	done

cli:
	cargo run --bin html5gum-fuzz-cli
