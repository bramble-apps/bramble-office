#!/bin/bash
# Build dist/Keydeck.app on a Mac from the release binary.
#   packaging/build-macos.sh [--install]
# --install also copies it to /Applications (~/Applications when /Applications isn't
# writable). Signed ad hoc, for the Mac it's built on (no notarization).
# Needs Xcode's command line tools (sips, iconutil, codesign) and Rust.
# CARGO_BUILD_TARGET=x86_64-apple-darwin (with MACOSX_DEPLOYMENT_TARGET=10.15) builds it for Intel Macs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="$(grep -m1 '^version' crates/kn-app/Cargo.toml | cut -d'"' -f2)"
cargo build --release -p kn-app
APP="$ROOT/dist/Keydeck.app"
rm -rf "${APP:?}"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
install -m755 "target/${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}release/keydeck" "$APP/Contents/MacOS/Keydeck"
strip "$APP/Contents/MacOS/Keydeck" || true
# the icon: every size of an .iconset from the 1024 px render, then .icns
SET="$ROOT/target/Keydeck.iconset"
rm -rf "${SET:?}"
mkdir -p "$SET"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" crates/kn-app/data/icons/keydeck-1024.png --out "$SET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) crates/kn-app/data/icons/keydeck-1024.png --out "$SET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$SET" -o "$APP/Contents/Resources/Keydeck.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Keydeck</string>
  <key>CFBundleDisplayName</key><string>Keydeck</string>
  <key>CFBundleIdentifier</key><string>app.keydeck.Keydeck</string>
  <key>CFBundleExecutable</key><string>Keydeck</string>
  <key>CFBundleIconFile</key><string>Keydeck</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>${MACOSX_DEPLOYMENT_TARGET:-11.0}</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDocumentTypes</key><array><dict>
    <key>CFBundleTypeName</key><string>Keynote Presentation</string>
    <key>CFBundleTypeRole</key><string>Editor</string>
    <key>LSItemContentTypes</key><array><string>com.apple.iwork.keynote.key</string><string>com.apple.iwork.keynote.sffkey</string></array>
    <key>CFBundleTypeExtensions</key><array><string>key</string></array>
    <!-- Keynote stays the default app for .key files; Keydeck is offered in Open With -->
    <key>LSHandlerRank</key><string>Alternate</string>
  </dict></array>
</dict></plist>
PLIST
codesign --force --deep -s - "$APP"
ls -la "$APP/Contents/MacOS"
if [ "${1:-}" = "--install" ]; then
  DEST=/Applications
  [ -w "$DEST" ] || DEST="$HOME/Applications"
  mkdir -p "$DEST"
  rm -rf "${DEST:?}/Keydeck.app"
  cp -R "$APP" "$DEST/"
  # only the installed copy stays registered (else Open With lists Keydeck twice)
  LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
  "$LSREG" -u "$APP" >/dev/null 2>&1 || true
  rm -rf "${APP:?}"
  "$LSREG" -f "$DEST/Keydeck.app" >/dev/null 2>&1 || true
  echo "installed $DEST/Keydeck.app"
fi
