#!/bin/sh
# Optional: install Letters for this user only (~/.local), so it shows in the app menu and opens
# .pages / .docx / .rtf files. Builds the release binary first. Undo: delete ~/.local/bin/letters,
# ~/.local/share/applications/letters.desktop and the letters icons in ~/.local/share/icons/hicolor.
set -e
cd "$(dirname "$0")/.."
cargo build --release -p pg-app
install -Dm755 target/release/letters "$HOME/.local/bin/letters"
# the app grid entry, launching the installed binary by its full path (the desktop's PATH may
# not include ~/.local/bin)
mkdir -p "$HOME/.local/share/applications"
sed "s|^Exec=letters|Exec=$HOME/.local/bin/letters|" packaging/letters.desktop > "$HOME/.local/share/applications/letters.desktop"
# the icon at every size, and the scalable one
for f in crates/pg-app/data/icons/letters-*.png; do
    z=$(basename "$f" .png | sed 's/letters-//')
    install -Dm644 "$f" "$HOME/.local/share/icons/hicolor/${z}x${z}/apps/letters.png"
done
install -Dm644 crates/pg-app/data/icons/letters.svg "$HOME/.local/share/icons/hicolor/scalable/apps/letters.svg"
gtk-update-icon-cache -q -t -f "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "Installed: ~/.local/bin/letters (make sure ~/.local/bin is on PATH)"
