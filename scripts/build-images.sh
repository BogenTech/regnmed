#!/bin/sh
# Cross-compiles regnmed + regnid natively on the Mac (fast, cached, no
# RAM spikes in the VM) and builds the two runtime images. Requires:
#   brew install filosottile/musl-cross/musl-cross
#   rustup target add aarch64-unknown-linux-musl x86_64-unknown-linux-musl
#
# Usage:
#   scripts/build-images.sh              # aarch64 — the colima dev cluster
#   scripts/build-images.sh x86_64       # the homelab nodes
#
# The homelab images normally come from CI (.github/workflows/images.yml
# pushes to ghcr.io/bogentech), which is reproducible and needs no local
# registry credential. This path exists for trying a build before pushing.
set -e
cd "$(dirname "$0")/.."

ARCH=${1:-aarch64}
case "$ARCH" in
    aarch64|arm64) ARCH=aarch64 ;;
    x86_64|amd64)  ARCH=x86_64 ;;
    *) echo "unknown arch '$ARCH' (want aarch64 or x86_64)" >&2; exit 1 ;;
esac

TARGET="$ARCH-unknown-linux-musl"

# musl-cross names its toolchain after the arch; cargo wants the linker in an
# env var whose name is the target triple upper-cased with dashes as
# underscores.
UPPER=$(echo "$TARGET" | tr 'a-z-' 'A-Z_')
export CARGO_TARGET_${UPPER}_LINKER="$ARCH-linux-musl-gcc"
export CC_$(echo "$TARGET" | tr '-' '_')="$ARCH-linux-musl-gcc"

if ! command -v "$ARCH-linux-musl-gcc" >/dev/null 2>&1; then
    echo "missing $ARCH-linux-musl-gcc — brew install filosottile/musl-cross/musl-cross" >&2
    echo "(the formula builds one arch at a time; x86_64 needs --with-x86_64)" >&2
    exit 1
fi

echo "==> building regnmed (api + cli) for $TARGET"
cargo build --release --target "$TARGET" -p regnmed-api -p regnmed-cli

echo "==> building regnid for $TARGET"
(cd ../regnid && cargo build --release --target "$TARGET")

echo "==> docker images"
docker build --build-arg TARGET="$TARGET" -t regnmed:dev .
docker build --build-arg TARGET="$TARGET" -t regnid:dev ../regnid

echo "done: images regnmed:dev and regnid:dev ($TARGET)"
