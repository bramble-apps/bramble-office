#!/bin/bash
# Build dist/Letters.app on a Mac from the release binary.
#   packaging/build-macos.sh [--install]
# --install also copies it to /Applications (~/Applications when /Applications isn't
# writable). Signed ad hoc, for the Mac it's built on (no notarization).
# Needs Xcode's command line tools (sips, iconutil, codesign) and Rust.
# CARGO_BUILD_TARGET=x86_64-apple-darwin (with MACOSX_DEPLOYMENT_TARGET=10.15) builds it for Intel Macs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="$(grep -m1 '^version' crates/pg-app/Cargo.toml | cut -d'"' -f2)"
cargo build --release -p pg-app
APP="$ROOT/dist/Letters.app"
rm -rf "${APP:?}"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
install -m755 "target/${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}release/letters" "$APP/Contents/MacOS/Letters"
strip "$APP/Contents/MacOS/Letters" || true
# the icon: every size of an .iconset from the 1024 px render, then .icns
SET="$ROOT/target/Letters.iconset"
rm -rf "${SET:?}"
mkdir -p "$SET"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" crates/pg-app/data/icons/letters-1024.png --out "$SET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) crates/pg-app/data/icons/letters-1024.png --out "$SET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$SET" -o "$APP/Contents/Resources/Letters.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Letters</string>
  <key>CFBundleDisplayName</key><string>Letters</string>
  <key>CFBundleIdentifier</key><string>app.letters.Letters</string>
  <key>CFBundleExecutable</key><string>Letters</string>
  <key>CFBundleIconFile</key><string>Letters</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>${MACOSX_DEPLOYMENT_TARGET:-11.0}</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDocumentTypes</key><array>
    <!-- Alternate: Letters opens these (Open With, Get Info) without becoming their default
         app in Pages' or Word's place -->
    <dict>
      <key>CFBundleTypeName</key><string>Pages Document</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSHandlerRank</key><string>Alternate</string>
      <key>LSItemContentTypes</key><array><string>com.apple.iwork.pages.pages</string><string>com.apple.iwork.pages.sffpages</string></array>
      <key>CFBundleTypeExtensions</key><array><string>pages</string></array>
    </dict>
    <dict>
      <key>CFBundleTypeName</key><string>Word Document</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSHandlerRank</key><string>Alternate</string>
      <key>LSItemContentTypes</key><array><string>org.openxmlformats.wordprocessingml.document</string></array>
      <key>CFBundleTypeExtensions</key><array><string>docx</string></array>
    </dict>
  </array>
</dict></plist>
PLIST
codesign --force --deep -s - "$APP"
ls -la "$APP/Contents/MacOS"
if [ "${1:-}" = "--install" ]; then
  DEST=/Applications
  [ -w "$DEST" ] || DEST="$HOME/Applications"
  mkdir -p "$DEST"
  rm -rf "${DEST:?}/Letters.app"
  cp -R "$APP" "$DEST/"
  # only the installed copy stays registered (else Open With lists Letters twice)
  LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
  "$LSREG" -u "$APP" >/dev/null 2>&1 || true
  rm -rf "${APP:?}"
  "$LSREG" -f "$DEST/Letters.app" >/dev/null 2>&1 || true
  echo "installed $DEST/Letters.app"
fi
