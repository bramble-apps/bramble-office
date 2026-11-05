#!/bin/bash
# Build dist/Bramble Notes.app on a Mac from the release binary.
#   packaging/build-macos.sh [--install]
# --install also copies it to /Applications (~/Applications when /Applications isn't
# writable). Signed ad hoc, for the Mac it's built on (no notarization).
# Needs Xcode's command line tools (sips, iconutil, codesign) and Rust.
# CARGO_BUILD_TARGET=x86_64-apple-darwin (with MACOSX_DEPLOYMENT_TARGET=10.15) builds it for Intel Macs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="$(grep -m1 '^version' crates/nt-app/Cargo.toml | cut -d'"' -f2)"
cargo build --release -p nt-app
APP="$ROOT/dist/Bramble Notes.app"
rm -rf "${APP:?}"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
install -m755 "target/${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}release/bramble-notes" "$APP/Contents/MacOS/BrambleNotes"
strip "$APP/Contents/MacOS/BrambleNotes" || true
# the icon: every size of an .iconset from the 1024 px render, then .icns
SET="$ROOT/target/BrambleNotes.iconset"
rm -rf "${SET:?}"
mkdir -p "$SET"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" crates/nt-app/data/icons/notes-1024.png --out "$SET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) crates/nt-app/data/icons/notes-1024.png --out "$SET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$SET" -o "$APP/Contents/Resources/BrambleNotes.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Bramble Notes</string>
  <key>CFBundleDisplayName</key><string>Bramble Notes</string>
  <key>CFBundleIdentifier</key><string>app.bramble.notes</string>
  <key>CFBundleExecutable</key><string>BrambleNotes</string>
  <key>CFBundleIconFile</key><string>BrambleNotes</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>${MACOSX_DEPLOYMENT_TARGET:-11.0}</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --deep -s - "$APP"
ls -la "$APP/Contents/MacOS"
if [ "${1:-}" = "--install" ]; then
  DEST=/Applications
  [ -w "$DEST" ] || DEST="$HOME/Applications"
  mkdir -p "$DEST"
  rm -rf "${DEST:?}/Bramble Notes.app"
  cp -R "$APP" "$DEST/"
  # only the installed copy stays registered (else Open With lists it twice)
  LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
  "$LSREG" -u "$APP" >/dev/null 2>&1 || true
  rm -rf "${APP:?}"
  "$LSREG" -f "$DEST/Bramble Notes.app" >/dev/null 2>&1 || true
  echo "installed $DEST/Bramble Notes.app"
fi
