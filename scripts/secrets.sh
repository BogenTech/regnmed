#!/usr/bin/env bash
# Dev secrets via SOPS + age (docs/secrets.md).
#
# The encrypted file (secrets/dev.enc.env) is committed; the decrypted
# twin (secrets/dev.env) is gitignored and never leaves the machine.
# Only DEV secrets belong here — see docs/secrets.md for why test and
# prod are handled differently.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENC="$REPO_ROOT/secrets/dev.enc.env"
PLAIN="$REPO_ROOT/secrets/dev.env"
KEYFILE="${SOPS_AGE_KEY_FILE:-$HOME/Library/Application Support/sops/age/keys.txt}"

die() { printf '%s\n' "$*" >&2; exit 1; }

need_tools() {
    command -v sops >/dev/null || die "sops not installed — brew install sops age"
    command -v age  >/dev/null || die "age not installed — brew install sops age"
}

need_key() {
    [ -f "$KEYFILE" ] || die "no age key at $KEYFILE
This machine has no key yet. Run:  $0 init
then add the printed public key to .sops.yaml on a machine that can
already decrypt (scripts/secrets.sh add-recipient <age1...>)."
    export SOPS_AGE_KEY_FILE="$KEYFILE"
}

case "${1:-help}" in
init)
    need_tools
    if [ -f "$KEYFILE" ]; then
        echo "Key already exists at $KEYFILE — not overwriting."
    else
        mkdir -p "$(dirname "$KEYFILE")"
        age-keygen -o "$KEYFILE" 2>/dev/null
        chmod 600 "$KEYFILE"
        echo "Generated $KEYFILE"
    fi
    echo
    echo "This machine's PUBLIC key (safe to share, add to .sops.yaml):"
    grep '^# public key:' "$KEYFILE" | sed 's/^# public key: //'
    ;;

decrypt)
    need_tools; need_key
    [ -f "$ENC" ] || die "no $ENC yet — nothing to decrypt"
    sops --decrypt --input-type dotenv --output-type dotenv "$ENC" > "$PLAIN"
    chmod 600 "$PLAIN"
    echo "Wrote $PLAIN ($(grep -cE '^[A-Za-z_]' "$PLAIN") variables)"
    ;;

encrypt)
    need_tools; need_key
    [ -f "$PLAIN" ] || die "no $PLAIN to encrypt"
    sops --encrypt --input-type dotenv --output-type dotenv "$PLAIN" > "$ENC"
    echo "Wrote $ENC — commit it"
    ;;

edit)
    # Edits in place without ever writing plaintext to disk.
    need_tools; need_key
    sops --input-type dotenv --output-type dotenv "$ENC"
    ;;

add-recipient)
    need_tools; need_key
    NEW="${2:-}"
    [ -n "$NEW" ] || die "usage: $0 add-recipient age1..."
    case "$NEW" in age1*) ;; *) die "not an age public key: $NEW" ;; esac
    grep -q "$NEW" "$REPO_ROOT/.sops.yaml" \
        && die "already a recipient"
    die "Add this to the 'keys:' list in .sops.yaml, then run '$0 rekey':
  - &<machine-name> $NEW"
    ;;

rekey)
    # Re-encrypts to whatever .sops.yaml now lists.
    need_tools; need_key
    [ -f "$ENC" ] || die "no $ENC yet"
    sops updatekeys --yes "$ENC"
    echo "Re-encrypted $ENC to the current recipient list"
    ;;

*)
    cat <<EOF
Dev secrets (SOPS + age) — docs/secrets.md

  $0 init                    generate this machine's age key, print public key
  $0 decrypt                 secrets/dev.enc.env -> secrets/dev.env (local only)
  $0 encrypt                 secrets/dev.env -> secrets/dev.enc.env (commit this)
  $0 edit                    edit encrypted file in \$EDITOR, no plaintext on disk
  $0 add-recipient age1...   instructions for adding another machine
  $0 rekey                   re-encrypt after changing .sops.yaml recipients

Only DEV secrets belong here. Prod credentials are created in the
cluster (docs/deploy.md); the Maskinporten key stays outside git
entirely (docs/gov.md).
EOF
    ;;
esac
