#!/usr/bin/env bash
# Builds one suite app as an Android APK: the app crate's library as the native library of a
# NativeActivity (java/app/bramble/BrambleActivity.java), packaged with the SDK's own tools.
#
#   build-apk.sh <workspace> <cargo package> <lib name> <app id> <label> <icon.png> <out.apk> [mime types]
#
# ABIS: Android ABIs to include (default arm64-v8a; x86_64 for Waydroid and Chromebooks)
# VERSION: the version name (default 0.1.0); the version code comes from it
# ANDROID_KEYSTORE / ANDROID_KEYSTORE_PASS: the signing key (default: made once in
#   ~/.config/bramble/android-release.jks; keep it, updates need the same key)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/env.sh"
ws="$(cd "$1" && pwd)"; pkg="$2"; lib="$3"; appid="$4"; label="$5"; icon="$(realpath "$6")"; out="$(realpath -m "$7")"; mimes="${8:-}"
ABIS="${ABIS:-arm64-v8a}"
VERSION="${VERSION:-0.1.0}"
IFS=. read -r v1 v2 v3 <<<"${VERSION%%-*}"
code=$(( ${v1:-0} * 1000000 + ${v2:-0} * 1000 + ${v3:-0} ))
BT="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)"
JAR="$(ls -d "$ANDROID_HOME"/platforms/android-3[4-9]/android.jar | sort -V | tail -1)"
export JAVA_HOME="${JAVA_HOME:-$HOME/development/jdk}" PATH="${JAVA_HOME:-$HOME/development/jdk}/bin:$PATH"
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT

# the native library, once per ABI
for abi in $ABIS; do
    case "$abi" in
        arm64-v8a) target=aarch64-linux-android ;;
        x86_64) target=x86_64-linux-android ;;
        *) echo "unknown ABI $abi" >&2; exit 1 ;;
    esac
    (cd "$ws" && cargo rustc --release --target "$target" -p "$pkg" --lib --crate-type cdylib)
    mkdir -p "$work/apk/lib/$abi"
    cp "$ws/target/$target/release/lib$lib.so" "$work/apk/lib/$abi/"
    "$TC/llvm-strip" --strip-debug "$work/apk/lib/$abi/lib$lib.so"
done

# the activity
mkdir -p "$work/classes" "$work/dex"
"$JAVA_HOME/bin/javac" -nowarn --release 11 -classpath "$JAR" -d "$work/classes" "$here"/java/app/bramble/*.java
"$BT/d8" --release --min-api "$ANDROID_API" --lib "$JAR" --output "$work/dex" $(find "$work/classes" -name '*.class')
cp "$work/dex/classes.dex" "$work/apk/"

# "Open with": the app's document types
filters=""
if [ -n "$mimes" ]; then
    data=""
    for m in $mimes; do data+="                <data android:mimeType=\"$m\" />"$'\n'; done
    for action in VIEW EDIT; do
        filters+="            <intent-filter>
                <action android:name=\"android.intent.action.$action\" />
                <category android:name=\"android.intent.category.DEFAULT\" />
                <category android:name=\"android.intent.category.BROWSABLE\" />
                <data android:scheme=\"content\" />
                <data android:scheme=\"file\" />
$data            </intent-filter>
"
    done
fi
cat > "$work/AndroidManifest.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="$appid">
    <uses-permission android:name="android.permission.INTERNET" />
    <uses-feature android:name="android.hardware.touchscreen" android:required="false" />
    <application android:label="$label" android:icon="@mipmap/ic_launcher" android:hasCode="true"
        android:extractNativeLibs="true" android:resizeableActivity="true"
        android:theme="@android:style/Theme.DeviceDefault.NoActionBar.Fullscreen">
        <activity android:name="app.bramble.BrambleActivity" android:exported="true" android:launchMode="singleTask"
            android:configChanges="orientation|screenSize|screenLayout|smallestScreenSize|keyboard|keyboardHidden|navigation|density|uiMode"
            android:windowSoftInputMode="adjustResize">
            <meta-data android:name="android.app.lib_name" android:value="$lib" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
$filters        </activity>
    </application>
</manifest>
EOF

# the icon, then the APK
for d in mdpi:48 hdpi:72 xhdpi:96 xxhdpi:144 xxxhdpi:192; do
    mkdir -p "$work/res/mipmap-${d%%:*}"
    python3 -c "
from PIL import Image; import sys
Image.open(sys.argv[1]).convert('RGBA').resize((int(sys.argv[2]),)*2, Image.LANCZOS).save(sys.argv[3])" "$icon" "${d##*:}" "$work/res/mipmap-${d%%:*}/ic_launcher.png"
done
"$BT/aapt2" compile --dir "$work/res" -o "$work/res.zip"
"$BT/aapt2" link -o "$work/base.apk" -I "$JAR" --manifest "$work/AndroidManifest.xml" "$work/res.zip" \
    --min-sdk-version "$ANDROID_API" --target-sdk-version 34 --version-code "$code" --version-name "$VERSION"
(cd "$work/apk" && zip -q -r "$work/base.apk" .)
"$BT/zipalign" -f -p 4 "$work/base.apk" "$work/aligned.apk"

# signing
KS="${ANDROID_KEYSTORE:-$HOME/.config/bramble/android-release.jks}"
if [ ! -f "$KS" ]; then
    mkdir -p "$(dirname "$KS")"
    pass="$(head -c 24 /dev/urandom | base64 | tr -d '/+=')"
    "$JAVA_HOME/bin/keytool" -genkeypair -keystore "$KS" -storepass "$pass" -keypass "$pass" -alias bramble \
        -keyalg RSA -keysize 4096 -validity 10000 -dname "CN=Bramble Office" >/dev/null
    printf '%s\n' "$pass" > "${KS%.jks}.pass"
    chmod 600 "$KS" "${KS%.jks}.pass"
fi
pass="${ANDROID_KEYSTORE_PASS:-$(cat "${KS%.jks}.pass")}"
mkdir -p "$(dirname "$out")"
"$BT/apksigner" sign --ks "$KS" --ks-pass "pass:$pass" --ks-key-alias bramble --out "$out" "$work/aligned.apk"
echo "built $out ($(du -h "$out" | cut -f1))"
