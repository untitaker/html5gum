# Confirm that really only the current html5gum should be fuzzed. This will
# find panics and segfaults at best, not any other wrong behavior.
export FUZZ_BASIC := 0
# Add latest released html5gum to fuzzing
export FUZZ_OLD_HTML5GUM := 0
# Add html5ever to fuzzing
export FUZZ_HTML5EVER := 0
# Ignore errors while diffing to paper over bugs in old html5gum
#           order = ignore sort order of errors relative to other non-error
#                   tokens (not relative to each other)
#               1 = ignore all errors
# any other value = assert exact token equality
export FUZZ_IGNORE_PARSE_ERRORS := 0
# CLI arguments to pass to AFL. useful for multiprocessing
export _AFL_OPTS := -M fuzzer01

AFL_TARGET_BIN=target-afl/release/html5gum-fuzz-afl

clean:
	rm -rf target target-afl in out

in:
	mkdir -p in
	curl https://www.nytimes.com/ > in/nytimes.html
	curl https://docs.sentry.io/ > in/sentrydocs.html
	cat ../tests/html5lib-tests/tokenizer/*.tests | jq -r '.tests[].input' > in/html5lib-tokenizer-tests.html

setup-afl:
	which cargo-afl || cargo install afl
	CARGO_TARGET_DIR=./target-afl/ cargo afl build --release --bin html5gum-fuzz-afl

afl: in setup-afl
	CARGO_TARGET_DIR=./target-afl/ AFL_AUTORESUME=1 cargo afl fuzz $$_AFL_OPTS -i in -o out ${AFL_TARGET_BIN}

afl-next: setup-afl
	set -e && for f in $$(echo out/*/crashes/id:* | sort); do \
		echo $$f; \
		if ! $(MAKE) cli < $$f; then \
			cargo afl tmin -i $$f -o /tmp/html5gum-mintest ${AFL_TARGET_BIN}; \
			echo new test input written to /tmp/html5gum-mintest; \
			echo ----; \
			echo "hexyl preview:"; \
			hexyl /tmp/html5gum-mintest || true; \
			echo ----; \
			echo "json string:"; \
			cat /tmp/html5gum-mintest | jq -Ra . || true; \
			echo ----; \
			exit 2; \
		else \
			rm $$f; \
		fi \
	done

afl-skip:
	set -e && for f in $$(echo out/*/crashes/id:* | sort); do \
		rm $$f; \
		break; \
	done

whatsup:
	cargo afl whatsup -s out/

cli:
	cargo run --bin html5gum-fuzz-cli
