# Keydeck for Android

Bramble Office's presentation app on Android: the desktop Keydeck's window made in Flutter, on its Rust engine (keynote-linux/crates/kn-app, ffi.rs).

- `tool/build_engine.sh` builds the engine into android/app/src/main/jniLibs (arm64-v8a, x86_64) and for Linux runs.
- `flutter build apk --release --target-platform android-arm64` makes the APK, signed with ~/.config/bramble/android-release.jks.
- `tool/adaptive_icon.py` makes the launcher icon layers.
