#!/bin/bash
# dist/keydeck-<version>-linux-x86_64.tar.gz: the app, icons and an installer that needs no
# root (it installs into ~/.local). Unpack, then run ./install.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="$(grep -m1 '^version' crates/kn-app/Cargo.toml | cut -d'"' -f2)"
cargo build --release -p kn-app
NAME="keydeck-${VERSION}-linux-x86_64"
STAGE="$ROOT/target/tarball/$NAME"
rm -rf "${STAGE:?}"
mkdir -p "$STAGE/icons"
install -m755 target/release/keydeck "$STAGE/keydeck"
strip "$STAGE/keydeck" || true
cp crates/kn-app/data/icons/keydeck-*.png crates/kn-app/data/icons/keydeck.svg "$STAGE/icons/"
cat > "$STAGE/install.sh" <<'INST'
#!/bin/bash
# Install Keydeck for this user (no root): ~/.local/bin/keydeck, icons, app-grid entry.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HOME/.local/bin"; SHARE="${XDG_DATA_HOME:-$HOME/.local/share}"
mkdir -p "$BIN" "$SHARE/applications"
install -m755 "$HERE/keydeck" "$BIN/keydeck"
for f in "$HERE"/icons/keydeck-*.png; do
  s="$(basename "$f" .png)"; s="${s#keydeck-}"
  install -Dm644 "$f" "$SHARE/icons/hicolor/${s}x${s}/apps/keydeck.png"
done
install -Dm644 "$HERE/icons/keydeck.svg" "$SHARE/icons/hicolor/scalable/apps/keydeck.svg"
cat > "$SHARE/applications/keydeck.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=Keydeck
GenericName=Presentation
Comment=Create and play presentations; opens and saves Keynote files
Exec=$BIN/keydeck %F
Icon=keydeck
Terminal=false
Categories=Office;Presentation;
MimeType=application/x-iwork-keynote-sffkey;application/vnd.apple.keynote;
StartupWMClass=keydeck
DESK
update-desktop-database "$SHARE/applications" 2>/dev/null || true
echo "Keydeck installed: $BIN/keydeck (find it in the app grid)"
INST
chmod +x "$STAGE/install.sh"
mkdir -p dist
tar -C "$ROOT/target/tarball" -czf "dist/$NAME.tar.gz" "$NAME"
ls -la "dist/$NAME.tar.gz"
