#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
WORK="$ROOT/.work"
REF="$WORK/ref/tmux-3.8"
SOURCE="$WORK/tmp/tmux-3.8.tar.gz"
BUILD="$WORK/tmp/tmux-3.8"
URL="https://github.com/tmux/tmux/releases/download/3.8/tmux-3.8.tar.gz"
EXPECTED="e79c699c7e949dccd0a4a125e17b8d1261e311b16979dbc8eb34542f3966d82e"

fail() {
    printf 'install-tmux-ref: %s\n' "$*" >&2
    exit 1
}

digest() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        fail "sha256sum or shasum is required"
    fi
}

version() {
    pkg-config --modversion "$1" 2>/dev/null || printf 'unavailable'
}

for tool in curl tar make pkg-config cc; do
    command -v "$tool" >/dev/null 2>&1 || fail "missing required tool: $tool"
done
pkg-config --exists libevent || fail "missing libevent development files"
pkg-config --exists libutf8proc || pkg-config --exists utf8proc || fail "missing utf8proc development files"

mkdir -p "$WORK/tmp" "$WORK/ref" "$REF"
export TMPDIR="$WORK/tmp"

if [ ! -f "$SOURCE" ]; then
    curl -fL --retry 3 --output "$SOURCE" "$URL" || fail "download failed"
fi

ACTUAL=$(digest "$SOURCE")
[ "$ACTUAL" = "$EXPECTED" ] || fail "source hash mismatch: $ACTUAL"

if [ -x "$REF/bin/tmux" ] && [ "$("$REF/bin/tmux" -V)" = "tmux 3.8" ] && [ -f "$REF/.configure-flags" ] && [ "$(cat "$REF/.configure-flags")" = "enable-utf8proc disable-jemalloc" ]; then
    :
else
    rm -rf "$BUILD" "$REF"
    mkdir -p "$BUILD" "$REF"
    tar -xzf "$SOURCE" -C "$BUILD" --strip-components=1
    (
        cd "$BUILD"
        ./configure --prefix="$REF" --enable-utf8proc --disable-jemalloc
        make -j"$(getconf _NPROCESSORS_ONLN 2>/dev/null || printf 2)"
        make install
    )
    printf 'enable-utf8proc disable-jemalloc\n' > "$REF/.configure-flags"
fi

[ "$("$REF/bin/tmux" -V)" = "tmux 3.8" ] || fail "reference is not tmux 3.8"

{
    printf 'version: %s\n' "$("$REF/bin/tmux" -V)"
    printf 'source: %s\n' "$URL"
    printf 'source-sha256: %s\n' "$EXPECTED"
    printf 'configure: --enable-utf8proc --disable-jemalloc --prefix=%s\n' "$REF"
    printf 'platform: %s\n' "$(uname -srm)"
    printf 'cc: %s\n' "$(cc --version 2>/dev/null | head -n 1)"
    printf 'libevent: %s\n' "$(version libevent)"
    printf 'utf8proc: %s\n' "$(version libutf8proc)"
    printf 'ncurses: %s\n' "$(version ncurses)"
    printf 'binary-sha256: %s\n' "$(digest "$REF/bin/tmux")"
} > "$REF/PROVENANCE"

cat "$REF/PROVENANCE"
