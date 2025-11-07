#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ARTIFACT_DIR="$ROOT_DIR/hosts/swift/Artifacts"
HEADER_SRC="$ROOT_DIR/ffi/include/mobx_rs_ffi.h"

mkdir -p "$ARTIFACT_DIR"

ensure_target() {
    local target="$1"
    if ! rustup target list --installed | grep -q "^${target}$"; then
        echo "Installing Rust target: $target"
        rustup target add "$target"
    fi
}

build_target() {
    local target="$1"
    echo "Building Rust library for $target"
    cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --features ffi --release --target "$target"
}

copy_artifact() {
    local target="$1"
    local destination="$2"
    mkdir -p "$(dirname "$destination")"
    cp "$ROOT_DIR/target/$target/release/libmobx_rs.a" "$destination"
}

# macOS (arm64 only)
MAC_ARM="aarch64-apple-darwin"
ensure_target "$MAC_ARM"
build_target "$MAC_ARM"
MAC_OUTPUT="$ARTIFACT_DIR/macos/libmobx_rs.a"
mkdir -p "$(dirname "$MAC_OUTPUT")"
cp "$ROOT_DIR/target/$MAC_ARM/release/libmobx_rs.a" "$MAC_OUTPUT"

echo "macOS arm64 library written to $MAC_OUTPUT"

# iOS device
IOS_DEVICE="aarch64-apple-ios"
ensure_target "$IOS_DEVICE"
build_target "$IOS_DEVICE"
copy_artifact "$IOS_DEVICE" "$ARTIFACT_DIR/ios/device/libmobx_rs.a"

echo "iOS device library written to $ARTIFACT_DIR/ios/device/libmobx_rs.a"

# iOS Simulator (arm64 + x86_64)
IOS_SIM_ARM="aarch64-apple-ios-sim"
ensure_target "$IOS_SIM_ARM"
build_target "$IOS_SIM_ARM"
SIM_OUTPUT="$ARTIFACT_DIR/ios/simulator/libmobx_rs.a"
mkdir -p "$(dirname "$SIM_OUTPUT")"
cp "$ROOT_DIR/target/$IOS_SIM_ARM/release/libmobx_rs.a" "$SIM_OUTPUT"

echo "iOS simulator (arm64) library written to $SIM_OUTPUT"

# Copy public header alongside artifacts
mkdir -p "$ARTIFACT_DIR/include"
cp "$HEADER_SRC" "$ARTIFACT_DIR/include/"
cat > "$ARTIFACT_DIR/include/module.modulemap" <<'MAP'
module MobxRSFFI {
    header "mobx_rs_ffi.h"
    export *
}
MAP

echo "Header copied to $ARTIFACT_DIR/include"
echo "All artifacts ready under $ARTIFACT_DIR"
