.PHONY: build test lint release site clean

build:
	cargo build --release

test:
	cargo test --all-targets

lint:
	cargo clippy --all-targets

release:
	sh scripts/build_release.sh

site:
	python3 -m http.server 8000 --directory site

clean:
	cargo clean
	rm -rf dist
