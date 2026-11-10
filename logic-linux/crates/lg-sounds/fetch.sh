#!/bin/sh
# Downloads Bramble Studio's sampled-instrument library (release asset), checks its SHA-256 and
# installs it.
#
#   crates/lg-sounds/fetch.sh [DEST]
#
# DEST defaults to ~/.local/share/bramble-studio/sounds (the app also looks in $BRAMBLE_SOUNDS,
# <exe dir>/sounds and /usr/share/bramble-studio/sounds). Override the source with
# BRAMBLE_SOUNDS_URL (a URL or a local file path) when testing a new archive.
set -eu

VERSION=1
URL="${BRAMBLE_SOUNDS_URL:-https://github.com/bramble-apps/bramble-office/releases/download/sounds-v${VERSION}/bramble-sounds-${VERSION}.tar}"
SHA256="094229944c2e20bbcf8338c5b14b1c11dbb6fc79ea6d57e8bcb3904e265ab0a3"

DEST="${1:-${XDG_DATA_HOME:-$HOME/.local/share}/bramble-studio/sounds}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

echo "Bramble sounds v$VERSION -> $DEST"
case "$URL" in
    http://*|https://*) curl -fL --retry 3 -o "$TMP/sounds.tar" "$URL" ;;
    *) cp "$URL" "$TMP/sounds.tar" ;;
esac

GOT="$(sha256sum "$TMP/sounds.tar" | cut -d' ' -f1)"
if [ "$GOT" != "$SHA256" ]; then
    echo "checksum mismatch: expected $SHA256, got $GOT" >&2
    exit 1
fi

# the archive holds one top-level folder, bramble-sounds-$VERSION/
mkdir -p "$TMP/x"
tar -xf "$TMP/sounds.tar" -C "$TMP/x" --no-same-owner --no-same-permissions
SRC="$TMP/x/bramble-sounds-$VERSION"
[ -f "$SRC/VERSION" ] || { echo "unexpected archive layout" >&2; exit 1; }

mkdir -p "$(dirname "$DEST")"
rm -rf "$DEST.new"
mv "$SRC" "$DEST.new"
rm -rf "$DEST"
mv "$DEST.new" "$DEST"
echo "installed $(du -sh "$DEST" | cut -f1) in $DEST"
