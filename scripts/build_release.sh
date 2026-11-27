#!/usr/bin/env sh
set -eu

out="${1:-dist}"
cd "$(dirname "$0")/.."

cargo build --release
mkdir -p "$out"
cp target/release/safeusb "$out/safeusb"
echo "built $out/safeusb"
