# Configuration of the test target, see README for description.
export FUZZ_BASIC := 0
export FUZZ_OLD_HTML5GUM := 0
export FUZZ_HTML5EVER := 0
export FUZZ_IGNORE_PARSE_ERRORS :=
export FUZZ_LOLHTML := 0
export FUZZ_SWC := 0

# CLI arguments to pass to AFL. useful for multiprocessing
export _AFL_OPTS := -M fuzzer01
# CLI arguments to cargo fuzz
export _LIBFUZZER_OPTS := -s none

export AFL_MAP_SIZE := 90753
AFL_TARGET_BIN=target-afl/release/main_afl

all:
	@echo "no default target"
	@false

reset:
	rm -rf in out corpus artifacts

clean: reset
	rm -rf target target-afl

in: Makefile
	mkdir -p in
	set -e && cat ../tests/custom-html5lib-tests/*.test \
		../tests/html5lib-tests/tokenizer/*.test | \
		jq -r '(if .tests == null then [] else .tests end) | .[].input | (. + "\nAFL_END\n")' | \
		csplit --quiet --suppress-matched --elide-empty-files --prefix in/html5lib-tokenizer-tests- \
		- "/AFL_END/" "{*}"

afl-requeue:
	i=0; set -e; for f in in/*; do \
		dir=out/requeue-$$(shuf -i1-1000 -n1)-$$i/queue; \
		mkdir -p $$dir; \
		cp -v $$f $$dir/id:000000,requeued; \
		i=$$(( i+1 )); \
	done
.PHONY: requeue

setup-afl:
	which cargo-afl || cargo install afl
	CARGO_TARGET_DIR=./target-afl/ cargo afl build --release --bin main_afl --features afl

afl: in setup-afl
	CARGO_TARGET_DIR=./target-afl/ AFL_AUTORESUME=1 cargo afl fuzz $$_AFL_OPTS -i in -o out ${AFL_TARGET_BIN}

afl-next: setup-afl
	set -e && for f in out/*/crashes*/id:*; do \
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
	# relies on the same order as in afl-next, see
	# https://serverfault.com/a/122743/520816
	set -e && for f in out/*/crashes/id:*; do \
		rm $$f; \
		break; \
	done

whatsup:
	cargo afl whatsup -s out/

cli:
	cargo run --bin main_cli

libfuzzer: in
	mkdir -p out/
	cargo +nightly fuzz run main_libfuzzer in/ out/ $$_LIBFUZZER_OPTS

sh:
	$$SHELL
