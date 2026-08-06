#!/bin/sh
# Builds the Svelte portal (#76). The generated ui/portal/dist/ is
# checked in and embedded in regnmed-api with include_dir, so cargo —
# including the cross-compile in build-images.sh — never needs Node.
# Run this after changing anything under ui/portal/src/.
set -e
cd "$(dirname "$0")/../ui/portal"
# Always a clean, lockfile-exact install — the same environment the CI
# `portal` job rebuilds in; the checked-in dist must match CI byte for
# byte. (The 2026-08-06 dist drift turned out to be Tailwind scanning
# the previous dist — see `@source not` in src/app.css — but the clean
# install stays: it removes the other way local and CI can diverge.)
npm ci
npm run build
