#!/bin/sh
# One app as an AppImage: appimage.sh <binary> <id> <Name> <comment> <categories> <mime types> <icon.png> <out>
# Bundles the libraries the app loads at start that a minimal system may lack (keyboard
# layouts, X11 cursors and input); the graphics drivers (Vulkan, OpenGL), X11, Wayland and the
# C library come from the system, as AppImage guidance says. Needs appimagetool ($APPIMAGETOOL).
set -eu
bin=$1; id=$2; name=$3; comment=$4; cats=$5; mimes=$6; icon=$7; out=$8
dir=$(mktemp -d)/$name.AppDir
mkdir -p "$dir/usr/bin" "$dir/usr/lib" "$dir/usr/share/applications" "$dir/usr/share/icons/hicolor/256x256/apps"
install -m755 "$bin" "$dir/usr/bin/$id"
cp "$icon" "$dir/$id.png"
cp "$icon" "$dir/usr/share/icons/hicolor/256x256/apps/$id.png"
cat > "$dir/$id.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=$name
Comment=$comment
Exec=$id %F
Icon=$id
Terminal=false
Categories=$cats
MimeType=$mimes
StartupWMClass=$id
DESK
cp "$dir/$id.desktop" "$dir/usr/share/applications/"
for lib in libxkbcommon.so.0 libxkbcommon-x11.so.0 libXcursor.so.1 libXrandr.so.2 libXi.so.6 libXrender.so.1 libXfixes.so.3 libwayland-cursor.so.0; do
  path=$(ldconfig -p | awk -v l="$lib" '$1 == l && /x86-64/ {print $NF; exit}')
  [ -n "$path" ] && cp -L "$path" "$dir/usr/lib/$lib"
done
cat > "$dir/AppRun" <<RUN
#!/bin/sh
HERE=\$(dirname "\$(readlink -f "\$0")")
export LD_LIBRARY_PATH="\$HERE/usr/lib\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
exec "\$HERE/usr/bin/$id" "\$@"
RUN
chmod +x "$dir/AppRun"
ARCH=x86_64 "${APPIMAGETOOL:-appimagetool}" "$dir" "$out"
