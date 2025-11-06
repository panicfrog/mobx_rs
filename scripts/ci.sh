#!/usr/bin/env bash
set -euo pipefail

echo "Running cargo fmt..."
cargo fmt --all

echo "Running cargo clippy..."
cargo clippy --all-targets --all-features -D warnings

echo "Running cargo test..."
cargo test --all
