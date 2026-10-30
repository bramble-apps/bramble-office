#!/bin/bash
# Build dist/keydeck_<version>_amd64.deb from the release binary.
#   packaging/build-deb.sh [--with-themes]
# --with-themes also ships ~/.local/share/keydeck/themes in /usr/share/keydeck/themes.
# (Keynote's theme artwork library is never packaged: it is Apple's.)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="$(grep -m1 '^version' crates/kn-app/Cargo.toml | cut -d'"' -f2)"
cargo build --release -p kn-app
STAGE="$ROOT/target/deb/keydeck_${VERSION}_amd64"
rm -rf "${STAGE:?}"
mkdir -p "$STAGE/DEBIAN" "$STAGE/usr/bin" "$STAGE/usr/share/applications" "$STAGE/usr/share/doc/keydeck"
install -m755 target/release/keydeck "$STAGE/usr/bin/keydeck"
strip "$STAGE/usr/bin/keydeck" || true
for s in 16 24 32 48 64 128 256 512; do
  install -Dm644 "crates/kn-app/data/icons/keydeck-$s.png" "$STAGE/usr/share/icons/hicolor/${s}x${s}/apps/keydeck.png"
done
install -Dm644 crates/kn-app/data/icons/keydeck.svg "$STAGE/usr/share/icons/hicolor/scalable/apps/keydeck.svg"
cat > "$STAGE/usr/share/applications/keydeck.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=Keydeck
GenericName=Presentation
Comment=Create and play presentations; opens and saves Keynote files
Exec=keydeck %F
Icon=keydeck
Terminal=false
Categories=Office;Presentation;
MimeType=application/x-iwork-keynote-sffkey;application/vnd.apple.keynote;
StartupWMClass=keydeck
Keywords=slides;slideshow;keynote;presentation;
DESK
if [ "${1:-}" = "--with-themes" ]; then
  mkdir -p "$STAGE/usr/share/keydeck/themes"
  cp "$HOME/.local/share/keydeck/themes/"*.key "$HOME/.local/share/keydeck/themes/"*.png "$STAGE/usr/share/keydeck/themes/"
  cp "$HOME/.local/share/keydeck/themes/PHOTO-CREDITS.tsv" "$STAGE/usr/share/doc/keydeck/" 2>/dev/null || true
fi
SIZE="$(du -sk "$STAGE/usr" | cut -f1)"
cat > "$STAGE/DEBIAN/control" <<CTRL
Package: keydeck
Version: $VERSION
Section: office
Priority: optional
Architecture: amd64
Installed-Size: $SIZE
Depends: libc6, libgcc-s1, libxkbcommon0, libwayland-client0, libvulkan1 | libgl1
Recommends: ffmpeg, curl
Suggests: yt-dlp
Maintainer: Bramble Office <180196315+StephanRD@users.noreply.github.com>
Description: Presentation app that reads and writes Keynote files
 Keydeck creates, edits and plays presentations and opens and saves
 Apple Keynote (.key) files, with tables, charts, builds, transitions,
 movies and web videos.
CTRL
mkdir -p dist
dpkg-deb --build --root-owner-group "$STAGE" "dist/keydeck_${VERSION}_amd64.deb"
ls -la "dist/keydeck_${VERSION}_amd64.deb"
