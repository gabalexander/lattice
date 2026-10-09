#!/bin/sh
# Installs lattice from its releases on GitHub:
#
#   curl -fsSL https://raw.githubusercontent.com/gabalexander/lattice/main/install.sh | sh
#
# All of these are optional:
#
#   LATTICE_VERSION      the release to install, like 0.1.0, rather than the latest
#   LATTICE_INSTALL_DIR  where to put lattice, rather than ~/.local/bin
#   LATTICE_RELEASES     where to download releases from, for a mirror or a test
#   LATTICE_DRY_RUN=1    say what would be installed, and stop there
#
# Everything happens in main, called on the last line, so a download cut off
# halfway never runs half a script.
#
# Adapted from crystal's install.sh (MIT).

set -eu

repo="gabalexander/lattice"
releases="${LATTICE_RELEASES:-https://github.com/$repo/releases}"
install_dir="${LATTICE_INSTALL_DIR:-$HOME/.local/bin}"

say() {
    printf 'lattice: %s\n' "$*"
}

fail() {
    say "$*" >&2
    exit 1
}

# The release built for this machine, like aarch64-apple-darwin.
detect_target() {
    case "$(uname -s)" in
        Darwin) os="apple-darwin" ;;
        Linux) os="unknown-linux-musl" ;;
        *) fail "there's no release for $(uname -s); build lattice from source instead" ;;
    esac
    case "$(uname -m)" in
        arm64 | aarch64) arch="aarch64" ;;
        x86_64 | amd64) arch="x86_64" ;;
        *) fail "there's no release for $(uname -m); build lattice from source instead" ;;
    esac
    # A shell running under Rosetta on an Apple silicon Mac says x86_64, but
    # the build to have is the one for the machine itself.
    if [ "$os" = "apple-darwin" ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null)" = "1" ]; then
        arch="aarch64"
    fi
    echo "$arch-$os"
}

# The version to install: LATTICE_VERSION, or else the latest release, which
# is where GitHub's /releases/latest sends you.
resolve_version() {
    if [ -n "${LATTICE_VERSION:-}" ]; then
        echo "${LATTICE_VERSION#v}"
        return
    fi
    url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$releases/latest") ||
        fail "couldn't reach $releases"
    tag="${url##*/}"
    case "$tag" in
        v*) echo "${tag#v}" ;;
        *) fail "there's no release at $releases yet" ;;
    esac
}

# The SHA-256 checksum of a file, with whichever tool this machine has.
sha256() {
    if command -v sha256sum > /dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

main() {
    command -v curl > /dev/null 2>&1 || fail "curl is needed to download lattice"
    target=$(detect_target)
    version=$(resolve_version)
    archive="lattice-$version-$target"
    url="$releases/download/v$version/$archive.tar.gz"

    if [ "${LATTICE_DRY_RUN:-}" = "1" ]; then
        say "would install lattice $version for $target"
        say "from $url"
        say "into $install_dir"
        return
    fi

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT

    say "downloading lattice $version for $target"
    curl -fsSL -o "$tmp/$archive.tar.gz" "$url" || fail "couldn't download $url"
    curl -fsSL -o "$tmp/$archive.tar.gz.sha256" "$url.sha256" ||
        fail "couldn't download $url.sha256"

    expected=$(cut -d ' ' -f 1 < "$tmp/$archive.tar.gz.sha256")
    actual=$(sha256 "$tmp/$archive.tar.gz")
    [ "$expected" = "$actual" ] || fail "the download doesn't match its checksum; try again"

    tar -xzf "$tmp/$archive.tar.gz" -C "$tmp"
    mkdir -p "$install_dir"
    # Removed first rather than written over: macOS kills a program whose
    # signed file changes under it, and a running server is one.
    rm -f "$install_dir/lattice"
    cp "$tmp/$archive/lattice" "$install_dir/lattice"
    chmod 755 "$install_dir/lattice"
    say "installed $install_dir/lattice"

    case ":$PATH:" in
        *":$install_dir:"*) ;;
        *) say "$install_dir isn't on your PATH yet: add it to run lattice" ;;
    esac

    # lattice runs Claude Code for everything it asks of a model.
    command -v claude > /dev/null 2>&1 ||
        say "lattice needs Claude Code: install it, log in, then run lattice doctor"
}

main "$@"
