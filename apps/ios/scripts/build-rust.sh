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
if [ "${CONFIGURATION:-Debug}" = Release ]; then
  rust_profile=release; rust_release=--release
  # Keep Rust line tables in the archive dSYM so native-engine crash reports can be symbolicated.
  export CARGO_PROFILE_RELEASE_DEBUG=1
fi
cargo build --locked -p fta-ios --target "$rust_target" $rust_release
# Fail packaging rather than silently dropping notices when an existing dependency version changes.
python3 "$SRCROOT/scripts/generate-notices.py" --check
# Reject missing translations or changed format placeholders before shipping a localized binary.
python3 "$SRCROOT/scripts/generate-localizations.py" --check
mkdir -p "$DERIVED_FILE_DIR"
cp "target/$rust_target/$rust_profile/libfta_ios.a" "$DERIVED_FILE_DIR/libfta_ios.a"
