# Secrets

Secrets are sorted into three tiers, and each tier is handled
differently. Encrypting everything the same way is the mistake this
document exists to prevent: it makes the dangerous secrets feel as safe
as the harmless ones.

| Tier | Example | Where it lives | Why |
| --- | --- | --- | --- |
| **1. Not actually secret** | `DATABASE_URL=postgres://regnmed:regnmed@localhost:5433/regnmed` | `.env.example`, committed plaintext | Localhost, throwaway password. Encrypting it buys nothing and costs readability. |
| **2. Dev secrets** | test-environment API tokens | `secrets/dev.enc.env`, **SOPS-encrypted, committed** | Must travel between the developer's own machines. Low blast radius, high convenience value. |
| **3. Test and production** | Maskinporten key, prod `JWT_SECRET`, prod DB password | **never in this repo, in any form** | Created directly in the cluster (docs/deploy.md); the Maskinporten key stays outside git and outside container images (docs/gov.md). |

## The carve-out, stated explicitly

docs/gov.md says **"No secrets in the repo, ever."** That rule was
written about Maskinporten production keys and it still holds for
tier 3, without exception.

Tier 2 is a deliberate, narrow carve-out: **encrypted dev secrets may be
committed.** The reasoning is that a SOPS-encrypted value is not a
secret in the repo — it is ciphertext whose key never enters the repo,
and the alternative (plaintext `.env` files synced through a consumer
cloud service) is strictly worse. The carve-out is written down here so
it is a decision someone made, not a rule that quietly eroded.

If a tier-3 secret is ever about to be added to `secrets/`, that is the
signal that the tiering is being violated — not that the file needs a
stronger cipher.

## How it works: SOPS + age

`sops` encrypts the **values** in `secrets/dev.env` and leaves the
variable names readable, so a diff still says which secret changed
without revealing what it changed to. `age` is the key mechanism
(modern replacement for GPG — no keyring, one line per recipient).

Two files, one committed:

- `secrets/dev.enc.env` — encrypted, **committed**.
- `secrets/dev.env` — decrypted plaintext, **gitignored**, local only.

### Recipients are per machine

Each machine generates its **own** age key and adds its **public** key
to `.sops.yaml`. The private key never travels — which removes the
chicken-and-egg problem every file-based scheme has ("how do I securely
move the key that protects my secrets?").

Setting up a second machine:

```bash
brew install sops age && ./scripts/secrets.sh init
```

That prints the machine's public key. On a machine that can already
decrypt, add it to the `keys:` list in `.sops.yaml`, run
`./scripts/secrets.sh rekey`, and commit. The new machine can now
decrypt; nothing secret was transmitted.

Removing a machine re-encrypts too — but anything it already decrypted
is out of your control, so **rotate the underlying secret as well**.

### Daily use

```bash
./scripts/secrets.sh decrypt
```

| Command | Does |
| --- | --- |
| `secrets.sh init` | generate this machine's key, print its public key |
| `secrets.sh decrypt` | `dev.enc.env` → `dev.env` (local only) |
| `secrets.sh encrypt` | `dev.env` → `dev.enc.env` (commit this) |
| `secrets.sh edit` | edit in `$EDITOR` with **no plaintext ever written to disk** |
| `secrets.sh rekey` | re-encrypt after changing recipients |

`edit` is the one to prefer: it decrypts into memory, opens the editor,
and re-encrypts on save.

The key file lives at
`~/Library/Application Support/sops/age/keys.txt` (override with
`SOPS_AGE_KEY_FILE`). **Back it up in a password manager, not in a
synced folder** — if you lose every copy, the encrypted files are
unrecoverable by design.

## What must never go in `secrets/`

- The Maskinporten private key (docs/gov.md) — production *or* test.
- Production database credentials — `deploy/prod` takes them
  out-of-band precisely so no credential appears in any rendered
  manifest (docs/deploy.md).
- Anything belonging to another system's production environment.

### Where the tier-3 secrets actually are

A secret correctly kept out of the repo is a secret nobody can find.
Recording the *location* is not recording the secret, so:

| Secret | Location | Notes |
| --- | --- | --- |
| Maskinporten **test** key | `~/.config/regnmed/maskinporten-test.pem` (+ `.pub.pem`), mode 600 | RSA 2048, generated 2026-07-24 with the Samarbeidsportalen setup. Nothing in the repo references it — `MASKINPORTEN_KEY_FILE` is set in the environment when needed. |
| Maskinporten prod key | does not exist yet | Blocked on the scope grant (docs/gov.md). |

**Per machine, not copied.** Maskinporten accepts several keys per
client, which is what `MASKINPORTEN_KID` selects. A second development
machine should generate its **own** keypair and register that public key
on the same client — the same principle as the age recipients above, and
for the same reason: a private key that never moves cannot be
intercepted in transit or forgotten in a sync folder.

If a copy must exist as a backup, encrypt it to the target machine's age
key (`age -r age1... -o maskinporten-test.pem.age maskinporten-test.pem`)
or put it in a password manager. Never in `secrets/`, never plaintext in
a synced folder.

## Known limitation

SOPS encrypts comments in dotenv files as well as values, so the
committed file shows `#ENC[...]` lines where comments were. Variable
names stay readable, which is what makes diffs useful; the comments are
recoverable on decrypt.
