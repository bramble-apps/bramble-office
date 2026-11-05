#!/usr/bin/env bash
# Builds Notes' engine (notes-linux/crates/nt-ffi over nt-core) for Android into android/app/src/main/jniLibs.
#   tool/build_engine.sh [abi…]   (default: arm64-v8a x86_64; NOTES_ENGINE: the notes-linux checkout)
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
notes="${NOTES_ENGINE:-$here/../notes-linux}"
. "$here/../keynote-linux/tools/android/env.sh"
for abi in ${@:-arm64-v8a x86_64}; do
    case "$abi" in
        arm64-v8a) target=aarch64-linux-android ;;
        x86_64) target=x86_64-linux-android ;;
        *) echo "unknown ABI $abi" >&2; exit 1 ;;
    esac
    (cd "$notes" && cargo rustc --release --target "$target" -p nt-ffi --lib --crate-type cdylib)
    mkdir -p "$here/android/app/src/main/jniLibs/$abi"
    cp "$notes/target/$target/release/libnotes_core.so" "$here/android/app/src/main/jniLibs/$abi/"
    "$TC/llvm-strip" --strip-debug "$here/android/app/src/main/jniLibs/$abi/libnotes_core.so"
done
ls -la "$here"/android/app/src/main/jniLibs/*/
