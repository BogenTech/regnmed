#!/usr/bin/env bash
# Wire the portal's OIDC login to a local regnid, so `cargo run -p
# regnmed-api` gets a real login instead of a dead "Logg inn" button.
# This is the dev-db.sh tier of SSO: no cluster, two cargo processes.
# The cluster equivalent is scripts/dev-cluster.sh (seed()).
#
# Idempotent: re-registering an existing user/client fails quietly.
#
#   scripts/dev-sso.sh            # register against the default port
#   PORT=8085 scripts/dev-sso.sh  # ...or another one
#
# Afterwards, run the two servers (regnid first — regnmed-api resolves
# OIDC discovery at startup and exits if the issuer is unreachable):
#
#   (cd ../regnid && cargo run -- serve)
#   cargo run -p regnmed-api
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REGNID_DIR="${REGNID_DIR:-$REPO_ROOT/../regnid}"

# The portal builds its redirect URI from location.origin, so the port is
# part of the registration: change it here and the client must be
# re-registered. 8082 rather than the historical 8080, which collides with
# colima/k3s port forwards on some machines.
PORT="${PORT:-8082}"
ISSUER_PORT="${ISSUER_PORT:-8081}"
ISSUER="http://127.0.0.1:$ISSUER_PORT"

DEV_EMAIL="${DEV_EMAIL:-admin@example.test}"
DEV_PASSWORD="${DEV_PASSWORD:-korrekt hest batteri stift}"

if [ ! -d "$REGNID_DIR" ]; then
    echo "regnid not found at $REGNID_DIR (set REGNID_DIR)" >&2
    exit 1
fi

echo "==> regnid: migrate"
(cd "$REGNID_DIR" && cargo run --quiet -- migrate)

echo "==> regnid: dev admin ($DEV_EMAIL)"
(cd "$REGNID_DIR" && cargo run --quiet -- add-user \
    --email "$DEV_EMAIL" --password "$DEV_PASSWORD" \
    --name "Admin" --admin) >/dev/null 2>&1 ||
    echo "    (already exists)"

# A public client: the portal runs PKCE in the browser and holds no
# secret. Both hostnames are registered because the redirect URI must
# match location.origin exactly, and either one may be typed.
echo "==> regnid: client regnmed-portal for port $PORT"
(cd "$REGNID_DIR" && cargo run --quiet -- add-client \
    --client-id regnmed-portal --name "regnmed portal" \
    --redirect-uri "http://localhost:$PORT/callback" \
    --redirect-uri "http://127.0.0.1:$PORT/callback" \
    --redirect-uri "http://localhost:$PORT/ny/callback" \
    --redirect-uri "http://127.0.0.1:$PORT/ny/callback" \
    --post-logout-redirect-uri "http://localhost:$PORT/" \
    --post-logout-redirect-uri "http://127.0.0.1:$PORT/" \
    --audience regnmed) >/dev/null 2>&1 ||
    echo "    (already registered — delete it in /admin/clients to change ports)"

cat <<EOF

Put this in .env (gitignored):

  OIDC_ISSUER=$ISSUER
  BIND_ADDR=127.0.0.1:$PORT

Then log in at http://localhost:$PORT/ as $DEV_EMAIL.
EOF
