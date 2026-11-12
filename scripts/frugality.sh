#!/usr/bin/env bash
# Frugality gate (ROADMAP M6, issue #28): "a service that grows fat
# fails the build." Competing on the least resources is a product
# principle, so it is enforced like any other invariant — in CI.
#
# Measures, against hard budgets:
#   1. Release binary sizes (regnmed-api, regnmed CLI, regnmed-agent).
#   2. The portal's first load: the gzipped files index.html names.
#   3. regnmed-api peak RSS: the server runs against a real Postgres,
#      serves a few hundred requests, and its high-water mark
#      (Linux: VmHWM; macOS: polled ps rss) must stay under the same
#      64 MB limit the k8s deployment enforces (deploy/local).
#
# Requires DATABASE_URL. Run locally: scripts/dev-db.sh, then
#   DATABASE_URL=... scripts/frugality.sh
set -euo pipefail
cd "$(dirname "$0")/.."

# Budgets sit ~1.5x above measured reality: tight enough to catch creep,
# loose enough for normal feature growth. RSS matches the container limit.
#
# Measured in CI 2026-07-29, after the release profile got thin LTO +
# codegen-units=1 (Cargo.toml): api 15 MB, cli 8 MB, dist 322 KB,
# RSS 10 MB. The binaries were 22/11 MB earlier that day on the default
# profile — the budgets were lowered to lock that reduction in, because
# a budget nobody tightens turns a one-time win into headroom for creep.
# (Fat LTO reached 13/7 MB but cost the frugality job ~1,5 minutter per
# kjøring; se avveiningen i Cargo.toml.)
API_BIN_BUDGET_MB=20
CLI_BIN_BUDGET_MB=12
# The hosted agent worker (docs/agent.md §8.3). It runs in the cluster as
# its own CronJob, so it gets a budget like the other two — measured
# 5,4 MB on 2026-10-29, and the gap is for the provider client, not for
# drift. It links regnmed-db and reqwest and little else; if it starts
# approaching the API's size, something has been pulled in that belongs
# behind the API rather than beside it.
AGENT_BIN_BUDGET_MB=8
API_RSS_BUDGET_MB=64 # keep equal to the container limit in deploy/local/regnmed-api.yaml
# The compiled Svelte portal (#76): what a browser must download, gzipped,
# before the first page can draw -- every /assets/ file index.html names
# (the entry JS and the CSS, plus any modulepreload Vite adds). Taken from
# index.html rather than a glob so a renamed entry cannot slip past.
#
# Changed 2026-11-12 from the TOTAL uncompressed dist (568 KB budget, last
# raised 544 -> 552 -> 568 in two weeks; main had reached 582 KB). The
# total summed every lazy section chunk, so it grew with every feature and
# measured nothing a user waits for; the embedded size is already inside
# the regnmed-api binary budget. Measured 2026-11-12: entry JS 39 KB gz +
# CSS 24 KB gz = 63 KB. The 24 lazy chunks (~109 KB gz) are deliberately
# NOT budgeted -- a section loads when it is opened, not before.
#
# The CSS keeps its own line because it answers the old question first:
# CSS growth without a theme change is a dependency nobody meant to add
# (all daisyUI themes are already enabled, `themes: all` in app.css).
PORTAL_FIRST_LOAD_BUDGET_KB=60
PORTAL_CSS_BUDGET_KB=32

: "${DATABASE_URL:?DATABASE_URL must point at a Postgres (scripts/dev-db.sh)}"

echo "==> building release binaries"
cargo build --release -p regnmed-api -p regnmed-cli -p regnmed-agent

fail=0
report() { # name value budget unit
  if [ "$2" -le "$3" ]; then
    echo "OK    $1: $2 $4 (budget $3 $4)"
  else
    echo "FAIL  $1: $2 $4 exceeds the budget of $3 $4"
    fail=1
  fi
}

size_mb() { # path -> MB rounded up
  local bytes
  bytes=$(stat -f%z "$1" 2>/dev/null || stat -c%s "$1")
  echo $(((bytes + 1024 * 1024 - 1) / 1024 / 1024))
}

report "regnmed-api binary" "$(size_mb target/release/regnmed-api)" "$API_BIN_BUDGET_MB" MB
report "regnmed cli binary" "$(size_mb target/release/regnmed)" "$CLI_BIN_BUDGET_MB" MB
report "regnmed-agent binary" "$(size_mb target/release/regnmed-agent)" "$AGENT_BIN_BUDGET_MB" MB

DIST=ui/portal/dist
first_load=$(grep -oE '(src|href)="/assets/[^"]+"' "$DIST/index.html" | sed -E 's|.*"/||; s|"$||')
[ -n "$first_load" ] || { echo "FAIL  $DIST/index.html names no /assets/ files"; exit 1; }
gz_total=0
gz_css=0
for f in $first_load; do
  b=$(gzip -9c "$DIST/$f" | wc -c) # a missing file aborts the script (set -e + pipefail)
  gz_total=$((gz_total + b))
  case $f in *.css) gz_css=$((gz_css + b)) ;; esac
done
report "portal first load, gzip -9 ($(echo $first_load | tr '\n' ' '))" $(((gz_total + 1023) / 1024)) "$PORTAL_FIRST_LOAD_BUDGET_KB" KB
report "portal CSS, gzip -9" $(((gz_css + 1023) / 1024)) "$PORTAL_CSS_BUDGET_KB" KB

echo "==> measuring regnmed-api peak RSS under load"
./target/release/regnmed migrate >/dev/null

JWKS_FILE=$(mktemp)
echo '{"keys":[]}' >"$JWKS_FILE"
PORT=8791
BIND_ADDR=127.0.0.1:$PORT \
  OIDC_ISSUER=https://frugality.invalid \
  OIDC_JWKS_FILE="$JWKS_FILE" \
  ./target/release/regnmed-api &
API_PID=$!
disown
trap 'kill $API_PID 2>/dev/null || true; rm -f "$JWKS_FILE"' EXIT

for _ in $(seq 1 50); do
  curl -sf "http://127.0.0.1:$PORT/health" >/dev/null && break
  sleep 0.2
done

# Exercise the hot unauthenticated paths: static portal, config, the
# public anchor feed, and rejected bearer tokens (the auth path).
MAX_RSS_KB=0
poll_rss() {
  local rss
  if [ -r "/proc/$API_PID/status" ]; then
    rss=$(awk '/VmHWM/ {print $2}' "/proc/$API_PID/status")
  else
    rss=$(ps -o rss= -p "$API_PID" | tr -d ' ')
  fi
  [ -n "$rss" ] && [ "$rss" -gt "$MAX_RSS_KB" ] && MAX_RSS_KB=$rss
  return 0
}
for i in $(seq 1 100); do
  curl -sf "http://127.0.0.1:$PORT/health" >/dev/null
  curl -sf "http://127.0.0.1:$PORT/" >/dev/null
  curl -sf "http://127.0.0.1:$PORT/sw.js" >/dev/null
  curl -sf "http://127.0.0.1:$PORT/portal-config" >/dev/null
  curl -sf "http://127.0.0.1:$PORT/anchors" >/dev/null
  curl -s -o /dev/null -H "authorization: Bearer bogus.$i" "http://127.0.0.1:$PORT/me"
  [ $((i % 10)) -eq 0 ] && poll_rss
done
poll_rss

RSS_MB=$(((MAX_RSS_KB + 1023) / 1024))
report "regnmed-api peak RSS" "$RSS_MB" "$API_RSS_BUDGET_MB" MB

if [ "$fail" -ne 0 ]; then
  echo
  echo "The frugality budget is part of the product (ROADMAP.md). Either"
  echo "make the service leaner, or raise the budget consciously in this"
  echo "script AND deploy/local — in its own reviewed commit."
  exit 1
fi
echo "==> frugality budget holds"
