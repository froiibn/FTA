#!/bin/bash
set -euo pipefail
# SDK/NDK are user-controlled tool installations, not fixed application state paths.
project_dir="$(cd "$(dirname "$0")" && pwd)"
repo_dir="$(cd "$project_dir/../.." && pwd)"
sdk_dir="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}"
ndk_dir="${ANDROID_NDK_HOME:-$sdk_dir/ndk/28.2.13676358}"
case "$(uname -s)" in Darwin) tool_host=darwin-x86_64 ;; Linux) tool_host=linux-x86_64 ;; *) echo 'Use a native Android NDK host toolchain' >&2; exit 1 ;; esac
tool_bin="$ndk_dir/toolchains/llvm/prebuilt/$tool_host/bin"
if [[ ! -x "$tool_bin/aarch64-linux-android29-clang" ]]; then
    echo 'Android NDK 28.2.13676358 is required; install it in SDK Manager or set ANDROID_NDK_HOME.' >&2
    exit 1
fi
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$tool_bin/aarch64-linux-android29-clang"
export CC_aarch64_linux_android="$tool_bin/aarch64-linux-android29-clang"
export AR_aarch64_linux_android="$tool_bin/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS='-C link-arg=-Wl,-z,max-page-size=16384'
"${CARGO:-$HOME/.cargo/bin/cargo}" build --locked --manifest-path "$project_dir/bridge/Cargo.toml" --target aarch64-linux-android
out_dir="$project_dir/app/build/generated/jniLibs/arm64-v8a"
mkdir -p "$out_dir"
"$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER" -std=c11 -Wall -Wextra -Werror -fPIC -shared \
 "$project_dir/native/bridge.c" "$repo_dir/target/aarch64-linux-android/debug/libfta_android.a" \
 -Wl,--gc-sections -Wl,--exclude-libs,ALL -Wl,--version-script,"$project_dir/native/exports.map" \
 -Wl,-z,max-page-size=16384 -Wl,-z,defs -ldl -llog -landroid -lm -o "$out_dir/libfta_android.so"
