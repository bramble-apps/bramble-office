#!/usr/bin/env bash
# Builds Keydeck's engine (keynote-linux/crates/kn-app, its library: the presentations, editing
# logic and slide drawing; ffi.rs is its C ABI) for Android into android/app/src/main/jniLibs,
# or for Linux runs into build/engine (run with LD_LIBRARY_PATH=build/engine).
#   tool/build_engine.sh [abi…]   (default: arm64-v8a x86_64; "linux" for the desktop library)
# KEYNOTE=<path> builds from another checkout of keynote-linux (a worktree).
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
kn="${KEYNOTE:-$here/../keynote-linux}"
for abi in ${@:-arm64-v8a x86_64}; do
    if [ "$abi" = linux ]; then
        (cd "$kn" && cargo rustc --release -p kn-app --lib --crate-type cdylib)
        mkdir -p "$here/build/engine"
        cp "$kn/target/release/libkn_app.so" "$here/build/engine/"
        continue
    fi
    . "$kn/tools/android/env.sh"
    case "$abi" in
        arm64-v8a) target=aarch64-linux-android ;;
        x86_64) target=x86_64-linux-android ;;
        *) echo "unknown ABI $abi" >&2; exit 1 ;;
    esac
    (cd "$kn" && cargo rustc --release --target "$target" -p kn-app --lib --crate-type cdylib)
    mkdir -p "$here/android/app/src/main/jniLibs/$abi"
    cp "$kn/target/$target/release/libkn_app.so" "$here/android/app/src/main/jniLibs/$abi/"
    "$TC/llvm-strip" --strip-debug "$here/android/app/src/main/jniLibs/$abi/libkn_app.so"
done
ls -la "$here"/android/app/src/main/jniLibs/*/ "$here/build/engine" 2>/dev/null || true
