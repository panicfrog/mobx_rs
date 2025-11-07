#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
HOSTS_DIR="$ROOT_DIR/hosts/swift"
ARTIFACT_DIR="$HOSTS_DIR/Artifacts"
XCFRAMEWORK_DIR="$ROOT_DIR/dist/MobxRS.xcframework"
OUTPUT_ZIP="${1:-$XCFRAMEWORK_DIR.zip}"

mkdir -p "$ROOT_DIR/dist"
rm -rf "$XCFRAMEWORK_DIR" "$OUTPUT_ZIP"

# 1. Build Rust static libs for required targets
"$ROOT_DIR/scripts/build_rust_artifacts.sh"

# 2. Package arm64 macOS static library into xcframework
MAC_LIB="$ARTIFACT_DIR/macos/libmobx_rs.a"
IOS_DEVICE_LIB="$ARTIFACT_DIR/ios/device/libmobx_rs.a"
IOS_SIM_LIB="$ARTIFACT_DIR/ios/simulator/libmobx_rs.a"
HEADER_DIR="$ARTIFACT_DIR/include"

if [ ! -f "$MAC_LIB" ] || [ ! -f "$IOS_DEVICE_LIB" ] || [ ! -f "$IOS_SIM_LIB" ]; then
  echo "Missing artifacts. Please run build script and ensure libs exist." >&2
  exit 1
fi

xcodebuild -create-xcframework \
  -library "$MAC_LIB" -headers "$HEADER_DIR" \
  -library "$IOS_DEVICE_LIB" -headers "$HEADER_DIR" \
  -library "$IOS_SIM_LIB" -headers "$HEADER_DIR" \
  -output "$XCFRAMEWORK_DIR"

(cd "$ROOT_DIR/dist" && zip -r "$(basename "$OUTPUT_ZIP")" "$(basename "$XCFRAMEWORK_DIR")")

echo "XCFramework written to $OUTPUT_ZIP"
