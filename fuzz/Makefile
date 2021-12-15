# Confirm that really only the current html5gum should be fuzzed. This will
# find panics and segfaults at best, not any other wrong behavior.
export FUZZ_BASIC := 0
# Add latest released html5gum to fuzzing
export FUZZ_OLD_HTML5GUM := 0
# Add html5ever to fuzzing
export FUZZ_HTML5EVER := 0
# CLI arguments to pass to AFL. useful for multiprocessing
export _AFL_OPTS := ""

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

afl-next:
	set -e && for f in $$(echo out/*/crashes/id:* | sort); do \
		echo $$f; \
		if ! $(MAKE) cli < $$f; then \
			cargo afl tmin -i $$f -o /tmp/html5gum-mintest ${AFL_TARGET_BIN}; \
			echo /tmp/html5gum-mintest; \
			echo ----; \
			cat -v /tmp/html5gum-mintest; \
			echo; \
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


cli:
	cargo run --bin html5gum-fuzz-cli
