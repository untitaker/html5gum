export FUZZ_BASIC := 0
export FUZZ_OLD_HTML5GUM := 0
export FUZZ_HTML5EVER := 0

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
			afl-tmin -i $$f -o /tmp/html5gum-mintest ${AFL_TARGET_BIN}; \
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
