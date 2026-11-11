#!/usr/bin/env bash
# Gives a cluster namespace its OWN Maskinporten key (docs/secrets.md).
#
# Run ON a cluster node (it uses `sudo kubectl`), e.g. from the dev Mac:
#
#   ssh bisand@mando 'bash -s' -- regnmed-test <client-id> < scripts/maskinporten-cluster-key.sh
#
# Same rule as scripts/maskinporten-key.sh: one key per machine, never
# copied. The private key is generated here, goes straight into the
# `maskinporten-credentials` secret, and the temp file is shredded — it
# never leaves the node. Only the PUBLIC half is printed, as a JWK to
# register on the same client in Samarbeidsportalen; its `kid` (the RFC
# 7638 thumbprint) is stored in the secret so the API selects this key.
#
# Secret keys: key.pem, client-id, kid (all read by deploy/test's
# regnmed-api patch). The client id counts as secret per docs/secrets.md.
set -euo pipefail

ns="${1:?usage: maskinporten-cluster-key.sh <namespace> <client-id>}"
client_id="${2:?usage: maskinporten-cluster-key.sh <namespace> <client-id>}"

if sudo -n kubectl -n "$ns" get secret maskinporten-credentials >/dev/null 2>&1; then
  echo "maskinporten-credentials already exists in $ns — delete it first to rotate" >&2
  exit 1
fi

d=$(mktemp -d)
chmod 700 "$d"
trap 'shred -u "$d"/* 2>/dev/null || true; rmdir "$d"' EXIT

openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$d/key.pem" 2>/dev/null
chmod 600 "$d/key.pem"
modulus=$(openssl rsa -in "$d/key.pem" -noout -modulus 2>/dev/null | cut -d= -f2)

jwk=$(python3 - "$modulus" <<'PY'
import base64, hashlib, json, sys
b64 = lambda b: base64.urlsafe_b64encode(b).rstrip(b"=").decode()
n, e = b64(bytes.fromhex(sys.argv[1])), "AQAB"
kid = b64(hashlib.sha256(json.dumps({"e": e, "kty": "RSA", "n": n}, separators=(",", ":")).encode()).digest())
print(json.dumps({"kty": "RSA", "e": e, "n": n, "kid": kid, "alg": "RS256", "use": "sig"}))
PY
)
kid=$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["kid"])' "$jwk")

sudo -n kubectl -n "$ns" create secret generic maskinporten-credentials \
  --from-file=key.pem="$d/key.pem" \
  --from-literal=client-id="$client_id" \
  --from-literal=kid="$kid" >/dev/null

echo "Created maskinporten-credentials in $ns (kid $kid)." >&2
echo "Register this PUBLIC key on the client in Samarbeidsportalen:" >&2
echo "$jwk"
