#!/usr/bin/env bash
# Builds Values (numbers-linux/crates/nm-app, its library: the engine and app logic) for Android into android/app/src/main/jniLibs.
#   tool/build_engine.sh [abi…]   (default: arm64-v8a x86_64; VALUES_ENGINE: the numbers-linux checkout)
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
numbers="${VALUES_ENGINE:-$here/../numbers-linux}"
. "$here/../keynote-linux/tools/android/env.sh"
for abi in ${@:-arm64-v8a x86_64}; do
    case "$abi" in
        arm64-v8a) target=aarch64-linux-android ;;
        x86_64) target=x86_64-linux-android ;;
        *) echo "unknown ABI $abi" >&2; exit 1 ;;
    esac
    (cd "$numbers" && cargo rustc --release --target "$target" -p nm-app --lib --crate-type cdylib)
    mkdir -p "$here/android/app/src/main/jniLibs/$abi"
    cp "$numbers/target/$target/release/libnm_app.so" "$here/android/app/src/main/jniLibs/$abi/"
    "$TC/llvm-strip" --strip-debug "$here/android/app/src/main/jniLibs/$abi/libnm_app.so"
done
ls -la "$here"/android/app/src/main/jniLibs/*/
