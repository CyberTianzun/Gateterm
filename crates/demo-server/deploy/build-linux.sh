#!/bin/sh
set -eu
# Run on Linux with a Rust musl target and musl-gcc installed. Output is git-ignored.
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target/demo-linux}"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc
cargo build --manifest-path "$root/Cargo.toml" --locked --release \
  --target x86_64-unknown-linux-musl -p gateterm-demo
artifact="$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/gateterm-demo"
file "$artifact"
if readelf -l "$artifact" | grep -q INTERP; then
  echo 'Expected a static Linux executable' >&2
  exit 1
fi
sha256sum "$artifact"
