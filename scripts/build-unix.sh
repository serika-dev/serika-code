#!/usr/bin/env bash
# Build and test Serika Code on Linux or macOS.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "Platform : $(uname -s) $(uname -m)"
echo "Rust     : $(rustc -V)"
echo "Cargo    : $(cargo -V)"

cargo test --workspace
cargo build --release

echo ""
echo "Built binaries:"
echo "  target/release/serika"
echo "  target/release/serika-desktop"
