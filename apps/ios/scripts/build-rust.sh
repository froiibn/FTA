#!/bin/sh
# Xcode selects the SDK; Rust must match the platform rather than the development Mac's architecture.
set -eu
export PATH="$HOME/.cargo/bin:$PATH"
cd "$SRCROOT/../.."
case "${PLATFORM_NAME:-iphonesimulator}" in
  iphoneos) rust_target=aarch64-apple-ios ;;
  iphonesimulator) rust_target=aarch64-apple-ios-sim ;;
  *) echo "Unsupported iOS platform" >&2; exit 1 ;;
esac
if [ "${CURRENT_ARCH:-arm64}" != arm64 ] && [ "${CURRENT_ARCH:-undefined_arch}" != undefined_arch ]; then
  echo "FTA iOS currently supports arm64 devices and Apple Silicon simulators" >&2; exit 1
fi
# Never download targets or mutate the user's toolchain as a build phase side effect.
rust_profile=debug
rust_release=
if [ "${CONFIGURATION:-Debug}" = Release ]; then rust_profile=release; rust_release=--release; fi
cargo build --locked -p fta-ios --target "$rust_target" $rust_release
mkdir -p "$DERIVED_FILE_DIR"
cp "target/$rust_target/$rust_profile/libfta_ios.a" "$DERIVED_FILE_DIR/libfta_ios.a"
