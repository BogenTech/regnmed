#!/bin/sh
# Builds the Svelte portal (#76). The generated ui/portal/dist/ is
# checked in and embedded in regnmed-api with include_dir, so cargo —
# including the cross-compile in build-images.sh — never needs Node.
# Run this after changing anything under ui/portal/src/.
set -e
cd "$(dirname "$0")/../ui/portal"
[ -d node_modules ] || npm install
npm run build
