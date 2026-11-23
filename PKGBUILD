# Maintainer: noahlyk <noahlykins@gmail.com>
#
# Local build of the checkout this file sits in (no source download, no AUR): `make pkg` or
# `makepkg -si`. `make pkg` keeps makepkg's work folders out of the repository's own src/.
pkgname=lyk-steno-theory
pkgver=0.1.0
pkgrel=1
pkgdesc="lyk steno theory: generates a Plover-format steno dictionary from phonetic rules (the lyk command)"
arch=('x86_64' 'aarch64')
url="https://github.com/noahlyk/lyk-steno-theory"
license=('GPL-3.0-or-later')
depends=('gcc-libs')
makedepends=('rust' 'cargo')
optdepends=('keymux: the steno engine `lyk deploy` writes into when installed (and `keymux steno validate` checks the files)'
            'typeladder: the typing trainer that `lyk deploy` gives the dictionary to when installed'
            'python: the measuring scripts in tools/ (checkout only)'
            'nodejs: the lesson site checks, site/check.mjs and site/test.mjs (checkout only)')
options=('!debug' '!lto')
install=lyk.install

source=()
sha256sums=()

# Local builds from a git checkout report the tag, commit count and hash, the same as `make`:
#   1.2.0+2.gbbd181e, or 1.2.0+2.gbbd181e.dirty with uncommitted changes (build.rs does the same).
# With no `v[0-9]*` tag yet: Cargo.toml's version and the hash, 0.1.0+gbbd181e. No git: pkgver above.
pkgver() {
    cd "$startdir"
    git rev-parse --git-dir >/dev/null 2>&1 || { echo "$pkgver"; return; }

    local version meta=() described
    if described=$(git describe --tags --long --match 'v[0-9]*' 2>/dev/null); then
        local rest="${described%-g*}"
        version="${rest%-*}"
        version="${version#v}"
        local count="${rest##*-}"
        (( count > 0 )) && meta+=("$count.g${described##*-g}")
    else
        version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
        meta+=("g$(git rev-parse --short HEAD)")
    fi
    [ -n "$(git status --porcelain --untracked-files=no)" ] && meta+=(dirty)

    if (( ${#meta[@]} )); then
        local IFS=.
        version+="+${meta[*]}"
    fi
    echo "$version"
}

build() {
    cd "$startdir"
    LYK_VERSION="$pkgver" cargo build --release --locked
}

package() {
    cd "$startdir"
    install -Dm755 "target/release/lyk" "$pkgdir/usr/bin/lyk"
    install -Dm644 "README.md" "$pkgdir/usr/share/doc/$pkgname/README.md"
    install -Dm644 "LICENSE" "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
    install -Dm644 "NOTICE" "$pkgdir/usr/share/licenses/$pkgname/NOTICE"
    # Nothing else is installed: the rules are compiled into the binary, and the data and output
    # folders are made by `lyk fetch` / `lyk build` (see `lyk where`).
}
