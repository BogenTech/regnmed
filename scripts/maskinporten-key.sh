#!/usr/bin/env bash
# Generate this machine's own Maskinporten keypair (docs/secrets.md).
#
# ONE KEY PER MACHINE, never copied. Maskinporten accepts several keys
# per client and `MASKINPORTEN_KID` selects between them, so a private
# key never has to travel — which also means it never has to be backed
# up: if a machine dies, the others keep working and its replacement
# generates a fresh key. Losing every machine at once is still
# recoverable, because registering a new key needs only ID-porten.
#
# The private key is written to ~/.config/regnmed/ (outside git, mode
# 600) and the PUBLIC half is printed as a JWK to paste into
# Samarbeidsportalen.
set -euo pipefail

ENVIRONMENT=test
NAME="$(scutil --get ComputerName 2>/dev/null || hostname -s)"
FORCE=0

usage() {
    cat <<EOF
Usage: $0 [--env test|prod] [--name <machine>] [--force]

Generates an RSA 2048 keypair for this machine and prints the public key
as a JWK for Samarbeidsportalen.

  --env    Maskinporten environment (default: test)
  --name   machine label used in filenames (default: this machine's name)
  --force  overwrite an existing key for this env+name

Registering the JWK is a manual paste today. Automating it needs a Digdir
administration client with scope idporten:dcr.write plus a
virksomhetssertifikat (servicedesk@digdir.no) — see docs/secrets.md.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --env)   ENVIRONMENT="${2:-}"; shift 2 ;;
        --name)  NAME="${2:-}"; shift 2 ;;
        --force) FORCE=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown argument: $1" >&2; usage >&2; exit 1 ;;
    esac
done

case "$ENVIRONMENT" in test|prod) ;; *) echo "--env must be test or prod" >&2; exit 1 ;; esac
# Filenames go into paths and env files; keep them boring.
SLUG="$(printf '%s' "$NAME" | tr '[:upper:] ' '[:lower:]-' | tr -cd 'a-z0-9-')"
[ -n "$SLUG" ] || { echo "--name produced an empty slug" >&2; exit 1; }

KEYDIR="$HOME/.config/regnmed"
BASE="$KEYDIR/maskinporten-$ENVIRONMENT-$SLUG"
KEY="$BASE.pem"
PUB="$BASE.pub.pem"
JWK="$BASE.jwk.json"

mkdir -p "$KEYDIR"
if [ -e "$KEY" ] && [ "$FORCE" -ne 1 ]; then
    echo "refusing to overwrite $KEY (use --force if you mean it)" >&2
    echo "a replaced key must also be removed in Samarbeidsportalen" >&2
    exit 1
fi

# PKCS#8 ("BEGIN PRIVATE KEY") — the format regnmed-gov is tested against.
umask 077
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$KEY" 2>/dev/null
chmod 600 "$KEY"
openssl pkey -in "$KEY" -pubout -out "$PUB"
chmod 644 "$PUB"

# Public key -> JWK. The kid we compute is the RFC 7638 thumbprint, which
# is unique per key without coordination — but Samarbeidsportalen assigns
# its OWN key id (a UUID) on registration, and that is the one that ends
# up in the token header. Read it back from the portal; see below.
python3 - "$PUB" "$JWK" <<'PY'
import base64, hashlib, json, re, subprocess, sys

pub_path, jwk_path = sys.argv[1], sys.argv[2]
text = subprocess.run(
    ["openssl", "pkey", "-pubin", "-in", pub_path, "-noout", "-text"],
    capture_output=True, text=True, check=True,
).stdout

m = re.search(r"Modulus:(.*?)Exponent:\s*(\d+)", text, re.S)
if not m:
    sys.exit("could not parse the public key")
modulus = bytes.fromhex(re.sub(r"[^0-9a-fA-F]", "", m.group(1)))
# OpenSSL prints a leading 0x00 sign byte; JWK wants the bare integer.
modulus = modulus.lstrip(b"\x00")
exponent = int(m.group(2))
exp_bytes = exponent.to_bytes((exponent.bit_length() + 7) // 8, "big")

b64 = lambda b: base64.urlsafe_b64encode(b).rstrip(b"=").decode()
n, e = b64(modulus), b64(exp_bytes)

# RFC 7638: SHA-256 over the canonical JSON of exactly kty/n/e.
thumb = hashlib.sha256(
    json.dumps({"e": e, "kty": "RSA", "n": n}, separators=(",", ":"), sort_keys=True).encode()
).digest()
kid = b64(thumb)

jwk = {"kty": "RSA", "use": "sig", "alg": "RS256", "kid": kid, "n": n, "e": e}
with open(jwk_path, "w") as fh:
    json.dump(jwk, fh, indent=2)
    fh.write("\n")
PY
KID="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["kid"])' "$JWK")"
chmod 644 "$JWK"

ENVFILE="$KEYDIR/maskinporten-$ENVIRONMENT.env"
cat <<EOF

Generated a keypair for "$NAME" ($ENVIRONMENT):

  private  $KEY          (mode 600 — never copy this to another machine)
  public   $PUB
  JWK      $JWK

  kid      $KID   (our thumbprint — see step 3)

NEXT — register the public key (needs only ID-porten):

  1. Open Samarbeidsportalen, find the "regnmed-$ENVIRONMENT" client,
     tab "Nøkler".
  2. "+ Legg til" and paste the contents of:
       $JWK
     KEEP the existing keys — other machines are using them, and this
     page is also where you "Slett" the key of a machine you have lost.
  3. Samarbeidsportalen assigns its OWN key id (a UUID) and shows it as
     the key's heading. Use THAT as the kid, not the thumbprint above.
     Then in $ENVFILE:
       MASKINPORTEN_KEY_FILE=$KEY
       MASKINPORTEN_KID=<the UUID Samarbeidsportalen shows>
  4. Note the expiry date the portal shows. Maskinporten keys expire
     (typically a couple of years) and a lapsed key fails at the worst
     possible moment — a frist. See docs/secrets.md.

No backup needed: this key belongs to this machine only. Lose the
machine, delete its key in the portal, run this script on the
replacement. Generating here rather than letting the portal generate
means the private key never crosses the network.
EOF
