#!/bin/sh
# Installs the prumo binary from the latest GitHub release. One line:
#
#   curl -fsSL https://github.com/kevinraymond/prumo/raw/main/install.sh | sh
#
# It picks the archive for this machine (Linux x86-64 or arm64, macOS Intel or Apple
# silicon), checks it against the sha256 published next to it, and puts `prumo` in
# ~/.local/bin. It says so if that directory is not on your PATH, and it replaces a `prumo`
# already there (the Docker wrapper, say). Nothing else is written.
#
#   PRUMO_VERSION      the release to install, e.g. v0.1.0 (default: the latest)
#   PRUMO_INSTALL_DIR  where the binary goes (default ~/.local/bin)
#   PRUMO_TARGET       the archive's target triple, when uname gets it wrong
#
# The same as flags: `sh -s -- --version v0.1.0 --dir /usr/local/bin`.
set -eu

repo="kevinraymond/prumo"
version="${PRUMO_VERSION:-}"
dir="${PRUMO_INSTALL_DIR:-$HOME/.local/bin}"
target="${PRUMO_TARGET:-}"

fail() { echo "install.sh: $*" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case "$1" in
        --version) version="$2"; shift 2 ;;
        --dir) dir="$2"; shift 2 ;;
        --target) target="$2"; shift 2 ;;
        -h|--help)
            echo "usage: install.sh [--version vX.Y.Z] [--dir DIR] [--target TRIPLE]"
            echo "Installs prumo from https://github.com/$repo/releases into DIR (default ~/.local/bin)."
            exit 0 ;;
        *) echo "install.sh: unknown option $1 (try --help)" >&2; exit 2 ;;
    esac
done

command -v curl >/dev/null 2>&1 || fail "curl is needed to download the release"
command -v tar >/dev/null 2>&1 || fail "tar is needed to unpack it"

if [ -z "$target" ]; then
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux)
            case "$arch" in
                x86_64|amd64) target="x86_64-unknown-linux-musl" ;;
                aarch64|arm64) target="aarch64-unknown-linux-musl" ;;
            esac ;;
        Darwin)
            case "$arch" in
                x86_64) target="x86_64-apple-darwin" ;;
                arm64) target="aarch64-apple-darwin" ;;
            esac ;;
        MINGW*|MSYS*|CYGWIN*)
            fail "on Windows, run this in PowerShell instead: irm https://github.com/$repo/raw/main/install.ps1 | iex" ;;
    esac
    [ -n "$target" ] || fail "no prebuilt binary for $os $arch. \`cargo install prumo\` builds one, and the Docker image runs where Docker does (see the README)"
fi

if [ -z "$version" ]; then
    # GitHub answers /releases/latest with a redirect to /releases/tag/vX.Y.Z.
    url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")" || fail "could not reach github.com"
    version="${url##*/}"
    case "$version" in
        v[0-9]*) ;;
        *) fail "could not tell the latest release from $url" ;;
    esac
fi

name="prumo-$version-$target"
base="https://github.com/$repo/releases/download/$version"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "prumo: downloading $name.tar.gz"
curl -fsSL -o "$tmp/$name.tar.gz" "$base/$name.tar.gz" \
    || fail "release $version has no $name.tar.gz (https://github.com/$repo/releases)"
curl -fsSL -o "$tmp/sum" "$base/$name.tar.gz.sha256" \
    || fail "release $version has no sha256 for $name.tar.gz"

want="$(tr -d ' \r\n' < "$tmp/sum")"
if command -v sha256sum >/dev/null 2>&1; then
    have="$(sha256sum "$tmp/$name.tar.gz" | cut -d' ' -f1)"
elif command -v shasum >/dev/null 2>&1; then
    have="$(shasum -a 256 "$tmp/$name.tar.gz" | cut -d' ' -f1)"
else
    fail "neither sha256sum nor shasum is here to check the download"
fi
[ "$have" = "$want" ] || fail "sha256 mismatch for $name.tar.gz: got $have, the release says $want"

tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
[ -f "$tmp/$name/prumo" ] || fail "$name.tar.gz has no prumo binary in it"

# Try the binary where it will live before it replaces anything there.
mkdir -p "$dir"
cp "$tmp/$name/prumo" "$dir/prumo.new"
chmod 755 "$dir/prumo.new"
if ! v="$("$dir/prumo.new" --version 2>/dev/null)"; then
    rm -f "$dir/prumo.new"
    fail "the $target binary does not run here. Wrong CPU or OS? PRUMO_TARGET picks another"
fi
replaced=""
[ -e "$dir/prumo" ] && replaced=" (replacing the one that was there)"
mv -f "$dir/prumo.new" "$dir/prumo"
echo "prumo: installed $v in $dir$replaced"

case ":$PATH:" in
    *":$dir:"*) ;;
    *)
        echo "prumo: $dir is not on your PATH. Add it in your shell's startup file:"
        echo "    export PATH=\"$dir:\$PATH\"" ;;
esac
echo "prumo: next, \`prumo doctor\` says what this machine can do"
