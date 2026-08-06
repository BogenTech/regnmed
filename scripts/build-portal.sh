#!/bin/sh
# Builds the Svelte portal (#76). The generated ui/portal/dist/ is
# checked in and embedded in regnmed-api with include_dir, so cargo —
# including the cross-compile in build-images.sh — never needs Node.
# Run this after changing anything under ui/portal/src/.
set -e
cd "$(dirname "$0")/../ui/portal"
# Always a clean, lockfile-exact install — the same environment the CI
# `portal` job rebuilds in. A stale node_modules once produced a dist
# 46 KB smaller than the reproducible build (2026-08-06), and the
# checked-in dist must match CI byte for byte.
npm ci
npm run build
