#!/bin/sh
# Optional: install Notes for this user only (~/.local), so it shows in the app menu. Builds the
# release binary first (Letters' packaging/install-local.sh). Undo: delete ~/.local/bin/bramble-notes,
# ~/.local/share/applications/bramble-notes.desktop and the bramble-notes icons in
# ~/.local/share/icons/hicolor.
set -e
cd "$(dirname "$0")/.."
cargo build --release -p nt-app
install -Dm755 target/release/bramble-notes "$HOME/.local/bin/bramble-notes"
# the app grid entry, launching the installed binary by its full path (the desktop's PATH may
# not include ~/.local/bin)
mkdir -p "$HOME/.local/share/applications"
sed "s|^Exec=bramble-notes|Exec=$HOME/.local/bin/bramble-notes|" packaging/bramble-notes.desktop > "$HOME/.local/share/applications/bramble-notes.desktop"
# the icon at every size
for f in crates/nt-app/data/icons/notes-*.png; do
    z=$(basename "$f" .png | sed 's/notes-//')
    install -Dm644 "$f" "$HOME/.local/share/icons/hicolor/${z}x${z}/apps/bramble-notes.png"
done
gtk-update-icon-cache -q -t -f "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "Installed: ~/.local/bin/bramble-notes (make sure ~/.local/bin is on PATH)"
