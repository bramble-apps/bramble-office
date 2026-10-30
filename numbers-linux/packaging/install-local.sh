#!/bin/sh
# Optional: install Values for this user only (~/.local), so it shows in the app menu and opens
# .numbers / .csv / .xlsx files. Builds the release binary first. Undo: delete ~/.local/bin/values,
# ~/.local/share/applications/numdeck.desktop and the numdeck icons in ~/.local/share/icons/hicolor.
set -e
cd "$(dirname "$0")/.."
cargo build --release -p nm-app
install -Dm755 target/release/values "$HOME/.local/bin/values"
# the app grid entry, launching the installed binary by its full path (the desktop's PATH may
# not include ~/.local/bin)
mkdir -p "$HOME/.local/share/applications"
sed "s|^Exec=values|Exec=$HOME/.local/bin/values|" packaging/numdeck.desktop > "$HOME/.local/share/applications/numdeck.desktop"
# the icon at every size, and the scalable one
for f in crates/nm-app/data/icons/numdeck-*.png; do
    z=$(basename "$f" .png | sed 's/numdeck-//')
    install -Dm644 "$f" "$HOME/.local/share/icons/hicolor/${z}x${z}/apps/numdeck.png"
done
install -Dm644 crates/nm-app/data/icons/numdeck.svg "$HOME/.local/share/icons/hicolor/scalable/apps/numdeck.svg"
gtk-update-icon-cache -q -t -f "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "Installed: ~/.local/bin/values (make sure ~/.local/bin is on PATH)"
