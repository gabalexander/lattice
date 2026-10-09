# Building and installing lattice. `make install` puts it in ~/.local/bin;
# `make install PREFIX=/usr/local` puts it in /usr/local/bin.

PREFIX ?= $(HOME)/.local
BIN := $(PREFIX)/bin

.PHONY: build release web test lint install uninstall clean

build:
	cargo build

# The web app first, when it's there, for the binary to carry it.
release: web
	cargo build --release

web:
	@if [ -f web/package.json ]; then npm --prefix web ci && npm --prefix web run build; fi

test:
	cargo test
	@if [ -f web/package.json ]; then npm --prefix web test; fi

lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	@if [ -f web/package.json ]; then npm --prefix web run lint --if-present && npm --prefix web run check; fi

# The old file is removed first, not written over, since macOS kills a
# program whose signed file changes under it.
install: release
	mkdir -p $(BIN)
	rm -f $(BIN)/lattice
	cp target/release/lattice $(BIN)/lattice

uninstall:
	rm -f $(BIN)/lattice

clean:
	cargo clean
	rm -rf web/build web/.svelte-kit
