# Local builds. The version comes from git (see build.rs):
#   tag v1.2.0 checked out       -> 1.2.0
#   8 commits past v1.2.0        -> 1.2.0+8.g0587d41
#   uncommitted changes          -> ...+dirty
#   no tag yet                   -> 0.1.0+g0587d41
# Release builds set LYK_VERSION explicitly, so they keep the plain release version.

PREFIX ?= /usr/local
BIN := target/release/lyk

.PHONY: all build install uninstall version clean check pkg

all: build

build:
	cargo build --release --locked

install: build
	install -Dm755 $(BIN) $(DESTDIR)$(PREFIX)/bin/lyk
	@echo "installed lyk $$($(BIN) --version | cut -d' ' -f2) to $(DESTDIR)$(PREFIX)/bin/lyk"

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/lyk

version: build
	@$(BIN) --version

clean:
	cargo clean

# What CI runs. Needs data/ (run `lyk fetch` first) and node.
check:
	cargo clippy --release --all-targets --locked -- -D warnings
	cargo test --release --locked -- --test-threads=4
	node site/check.mjs
	node site/test.mjs
	cargo run --release --locked -- score --check-readme

# An Arch package of this checkout into dist/. makepkg's work folders go under target/ because
# its default, src/, is this repository's own.
pkg:
	BUILDDIR=$(CURDIR)/target/makepkg PKGDEST=$(CURDIR)/dist makepkg -f
