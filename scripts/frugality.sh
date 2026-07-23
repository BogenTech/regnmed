#!/usr/bin/env bash
# Frugality gate (ROADMAP M6, issue #28): "a service that grows fat
# fails the build." Competing on the least resources is a product
# principle, so it is enforced like any other invariant — in CI.
#
# Measures, against hard budgets:
#   1. Release binary sizes (regnmed-api, regnmed CLI).
#   2. regnmed-api peak RSS: the server runs against a real Postgres,
#      serves a few hundred requests, and its high-water mark
#      (Linux: VmHWM; macOS: polled ps rss) must stay under the same
#      64 MB limit the k8s deployment enforces (deploy/local).
#
# Requires DATABASE_URL. Run locally: scripts/dev-db.sh, then
#   DATABASE_URL=... scripts/frugality.sh
set -euo pipefail
cd "$(dirname "$0")/.."

# Budgets are ~2x today's measured reality (11 MB / 8 MB / 11 MB) so
# they catch creep, not normal growth. RSS matches the container limit.
API_BIN_BUDGET_MB=24
CLI_BIN_BUDGET_MB=20
API_RSS_BUDGET_MB=64 # keep equal to the container limit in deploy/local/regnmed-api.yaml

: "${DATABASE_URL:?DATABASE_URL must point at a Postgres (scripts/dev-db.sh)}"

echo "==> building release binaries"
cargo build --release -p regnmed-api -p regnmed-cli

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
  curl -sf "http://127.0.0.1:$PORT/app.js" >/dev/null
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
