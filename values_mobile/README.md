# Values for Android

Bramble Office's spreadsheet app (Numbers, Excel, CSV) as a Flutter app on the desktop Values'
Rust engine (`numbers-linux/crates/nm-app`, its C ABI in `src/ffi.rs`). Everything on screen is
Flutter; the engine is the document library: open and save, the sheet's tiles, hit tests, the
selection and the cells, edit commands, formulas.

- `tool/build_engine.sh [abi…]` builds `libnm_app.so` into `android/app/src/main/jniLibs`
  (`VALUES_ENGINE=<numbers-linux checkout>`; default `../numbers-linux`).
- Linux: build the engine for the host (`cargo rustc --release -p nm-app --lib --crate-type cdylib`)
  and run with `LD_LIBRARY_PATH` pointing at its `target/release`.
- Checks: `VALUES_SNAP=<dir>`, `VALUES_SCRIPT="tap x y; drag …; type …; key …; snap name; quit"`,
  `VALUES_PHONE=WxH`, `VALUES_KEYBOARD=<points>`, `VALUES_OPEN=<file>` (test_snap.dart, main.dart).
- Android release: `flutter build apk --release --target-platform android-arm64`, signed with
  `~/.config/bramble/android-release.jks` (android/app/build.gradle.kts).
