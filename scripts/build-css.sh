#!/bin/sh
# Builds the portal stylesheet. The generated
# crates/regnmed-api/portal/app.css is checked in, so cargo never needs
# Node — run this only after changing portal markup/classes or themes.
set -e
cd "$(dirname "$0")/../ui"
[ -d node_modules ] || npm install
npx @tailwindcss/cli -i input.css -o ../crates/regnmed-api/portal/app.css --minify
