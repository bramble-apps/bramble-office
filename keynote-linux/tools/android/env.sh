# Sourced by the Android build scripts: the NDK's compilers and linkers for Rust's Android targets.
# ANDROID_HOME (the SDK, with an NDK installed under ndk/) defaults to ~/development/android-sdk.
export ANDROID_HOME="${ANDROID_HOME:-$HOME/development/android-sdk}"
NDK="${ANDROID_NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)}"
export ANDROID_NDK_HOME="$NDK"
ANDROID_API="${ANDROID_API:-26}"
TC="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
for t in aarch64-linux-android x86_64-linux-android; do
    u=$(echo "$t" | tr 'a-z-' 'A-Z_'); l=$(echo "$t" | tr '-' '_')
    export "CARGO_TARGET_${u}_LINKER=$TC/$t$ANDROID_API-clang"
    export "CC_$l=$TC/$t$ANDROID_API-clang" "CXX_$l=$TC/$t$ANDROID_API-clang++" "AR_$l=$TC/llvm-ar"
done
