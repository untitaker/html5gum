setup:
	which cargo-afl || cargo install afl
	cargo afl build
	mkdir -p in
	curl https://www.nytimes.com/ > in/nytimes.html
	curl https://docs.sentry.io/ > in/sentrydocs.html

run:
	cargo afl fuzz -i in -o out target/debug/html5gum-fuzz
