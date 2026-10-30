#!/bin/sh
# Add Keydeck to the desktop's app grid (per user): the icon in every size, and a launcher
# for the release build that opens .key files. Run again after moving the checkout.
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
for f in "$root"/crates/kn-app/data/icons/keydeck-*.png; do
    z=$(basename "$f" .png | sed 's/keydeck-//')
    mkdir -p "$data/icons/hicolor/${z}x${z}/apps"
    cp "$f" "$data/icons/hicolor/${z}x${z}/apps/keydeck.png"
done
mkdir -p "$data/icons/hicolor/scalable/apps" "$data/applications"
cp "$root/crates/kn-app/data/icons/keydeck.svg" "$data/icons/hicolor/scalable/apps/keydeck.svg"
cat > "$data/applications/keydeck.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Keydeck
GenericName=Presentation
Comment=Create and play presentations; opens and saves Keynote files
Exec="$root/target/release/keydeck" %F
Icon=keydeck
Terminal=false
Categories=Office;Presentation;
MimeType=application/x-iwork-keynote-sffkey;application/vnd.apple.keynote;
StartupWMClass=keydeck
Keywords=slides;slideshow;keynote;presentation;
DESKTOP
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$data/applications" 2>/dev/null || true
echo "Keydeck added to the app grid"
