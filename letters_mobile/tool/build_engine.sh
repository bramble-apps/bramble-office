#!/usr/bin/env bash
# Builds Letters (pages-linux/crates/pg-app, its library: the engine and app logic) for Android into android/app/src/main/jniLibs.
#   tool/build_engine.sh [abi…]   (default: arm64-v8a x86_64)
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
pages="$here/../pages-linux"
. "$here/../keynote-linux/tools/android/env.sh"
for abi in ${@:-arm64-v8a x86_64}; do
    case "$abi" in
        arm64-v8a) target=aarch64-linux-android ;;
        x86_64) target=x86_64-linux-android ;;
        *) echo "unknown ABI $abi" >&2; exit 1 ;;
    esac
    (cd "$pages" && cargo rustc --release --target "$target" -p pg-app --lib --crate-type cdylib)
    mkdir -p "$here/android/app/src/main/jniLibs/$abi"
    cp "$pages/target/$target/release/libpg_app.so" "$here/android/app/src/main/jniLibs/$abi/"
    "$TC/llvm-strip" --strip-debug "$here/android/app/src/main/jniLibs/$abi/libpg_app.so"
done
ls -la "$here"/android/app/src/main/jniLibs/*/
