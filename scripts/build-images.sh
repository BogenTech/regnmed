#!/bin/sh
# Cross-compiles regnmed + regnid natively on the Mac (fast, cached, no
# RAM spikes in the VM) and builds the two runtime images. Requires:
#   brew install filosottile/musl-cross/musl-cross
#   rustup target add aarch64-unknown-linux-musl
set -e
cd "$(dirname "$0")/.."

TARGET=aarch64-unknown-linux-musl
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-musl-gcc
export CC_aarch64_unknown_linux_musl=aarch64-linux-musl-gcc

echo "==> building regnmed (api + cli) for $TARGET"
cargo build --release --target "$TARGET" -p regnmed-api -p regnmed-cli

echo "==> building regnid for $TARGET"
(cd ../regnid && cargo build --release --target "$TARGET")

echo "==> docker images"
docker build -t regnmed:dev .
docker build -t regnid:dev ../regnid

echo "done: images regnmed:dev and regnid:dev"
