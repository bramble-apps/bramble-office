#!/bin/sh
# Install (or update) the whole Bramble Office suite - Keydeck, Letters and Values - from the
# latest GitHub release, with menu shortcuts. One command:
#   curl -fsSL https://raw.githubusercontent.com/bramble-apps/bramble-office/main/install.sh | sh
# Remove it again:
#   curl -fsSL https://raw.githubusercontent.com/bramble-apps/bramble-office/main/install.sh | sh -s -- --uninstall
#
# Linux (x86_64): the AppImages are unpacked into ~/.local/share/bramble-office (so they run
# without FUSE), with launchers in ~/.local/bin and app-menu entries with icons.
# macOS: the three apps go into ~/Applications.
# Nothing needs root; run it again to update.
set -eu

REPO=bramble-apps/bramble-office
URL="https://github.com/$REPO/releases/latest/download"
APPS="Keydeck Letters Values"

say() { printf '%s\n' "$*"; }
fail() { say "Bramble Office: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "needs '$1' (install it and run this again)"; }

fetch() { # url file
    if command -v curl >/dev/null 2>&1; then curl -fL --progress-bar -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then wget -q --show-progress -O "$2" "$1"
    else fail "needs curl or wget"; fi
}

uninstall=false
[ "${1:-}" = "--uninstall" ] && uninstall=true

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

case "$(uname -s)" in
Linux)
    [ "$(uname -m)" = x86_64 ] || fail "the Linux builds are for x86_64 PCs only (this is $(uname -m))"
    data=${XDG_DATA_HOME:-$HOME/.local/share}
    dest=$data/bramble-office
    bin=$HOME/.local/bin
    apps_dir=$data/applications
    icons=$data/icons/hicolor/256x256/apps
    if $uninstall; then
        for a in $APPS; do
            id=$(printf '%s' "$a" | tr 'A-Z' 'a-z')
            rm -f "$bin/$id" "$apps_dir/bramble-$id.desktop" "$icons/bramble-$id.png"
        done
        rm -rf "$dest"
        command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$apps_dir" || true
        say "Bramble Office removed (your documents are untouched)."
        exit 0
    fi
    mkdir -p "$dest" "$bin" "$apps_dir" "$icons"
    for a in $APPS; do
        id=$(printf '%s' "$a" | tr 'A-Z' 'a-z')
        say "Downloading $a..."
        fetch "$URL/$a-x86_64.AppImage" "$tmp/$a.AppImage"
        chmod +x "$tmp/$a.AppImage"
        # (unpacked: AppImages otherwise need FUSE 2, which newer Ubuntus don't have)
        (cd "$tmp" && "./$a.AppImage" --appimage-extract >/dev/null 2>&1) || fail "couldn't unpack $a"
        rm -rf "${dest:?}/$id"
        mv "$tmp/squashfs-root" "$dest/$id"
        rm -f "$tmp/$a.AppImage"
        # the launcher in ~/.local/bin, so `letters file.pages` works in a terminal
        printf '#!/bin/sh\nexec "%s/AppRun" "$@"\n' "$dest/$id" > "$bin/$id"
        chmod +x "$bin/$id"
        # icon and menu entry (the AppImage's own, pointed at the unpacked app)
        png=$(ls "$dest/$id"/*.png 2>/dev/null | head -n 1)
        [ -n "$png" ] && cp "$png" "$icons/bramble-$id.png"
        src=$(ls "$dest/$id"/*.desktop | head -n 1)
        sed -e "s|^Exec=[^ ]*|Exec=\"$dest/$id/AppRun\"|" -e "s|^Icon=.*|Icon=bramble-$id|" -e '/^TryExec=/d' "$src" > "$apps_dir/bramble-$id.desktop"
        say "  installed $a"
    done
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$apps_dir" || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
    say ""
    say "Bramble Office is installed: Keydeck, Letters and Values are in your app menu."
    case ":$PATH:" in *":$bin:"*) ;; *) say "(To start them from a terminal too, add $bin to your PATH.)" ;; esac
    ;;
Darwin)
    dest=$HOME/Applications
    if $uninstall; then
        for a in $APPS; do rm -rf "${dest:?}/$a.app"; done
        say "Bramble Office removed (your documents are untouched)."
        exit 0
    fi
    need unzip
    case "$(uname -m)" in arm64) zip=Bramble-Office-macos-apple-silicon.zip ;; *) zip=Bramble-Office-macos-intel.zip ;; esac
    say "Downloading Bramble Office for macOS..."
    fetch "$URL/$zip" "$tmp/suite.zip"
    unzip -q "$tmp/suite.zip" -d "$tmp"
    mkdir -p "$dest"
    for a in $APPS; do
        rm -rf "${dest:?}/$a.app"
        mv "$tmp/bramble-office/$a.app" "$dest/"
        # (downloaded by a script, not a browser: no quarantine prompt, but clear it in case)
        xattr -dr com.apple.quarantine "$dest/$a.app" 2>/dev/null || true
        say "  installed $a"
    done
    say ""
    say "Bramble Office is installed in $dest: find Keydeck, Letters and Values in Launchpad or Spotlight."
    ;;
*)
    fail "on Windows, run this in PowerShell instead:  irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex"
    ;;
esac
