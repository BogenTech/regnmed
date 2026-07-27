# Secrets

**Nothing secret lives in this repository, in any form — not even
encrypted.** docs/gov.md's "No secrets in the repo, ever" is absolute,
for test as well as production, and there is no carve-out.

That is affordable because regnmed barely has any secrets. What looks
like configuration mostly is configuration:

| What | Where | Secret? |
| --- | --- | --- |
| `DATABASE_URL`, `OIDC_ISSUER`, `OIDC_AUDIENCE`, `BIND_ADDR` | `.env`, from `.env.example` | **No.** Localhost, throwaway dev password. Committed as an example on purpose. |
| Maskinporten test key + client id | `~/.config/regnmed/` (see below) | **Yes** — the only one. |
| Production database credentials | Created in the cluster, out-of-band | **Yes.** `deploy/prod` is built so no credential appears in any rendered manifest (docs/deploy.md). |

A new machine gets a working dev environment with
`cp .env.example .env` and `docker compose up -d`. Nothing needs to be
carried across.

## Where the one real secret is

A secret correctly kept out of the repo is a secret nobody can find
again. Recording the *location* is not recording the secret:

```
~/.config/regnmed/maskinporten-test.pem       RSA 2048 private key, mode 600
~/.config/regnmed/maskinporten-test.pub.pem   public half (registered in Samarbeidsportalen)
~/.config/regnmed/maskinporten-test.env       MASKINPORTEN_* config incl. client id
```

Generated 2026-07-24 alongside the Samarbeidsportalen setup. Nothing in
the repo references these paths — `regnmed-gov` reads
`MASKINPORTEN_KEY_FILE` and friends from the environment, so you source
the env file when you need them:

```bash
set -a; . ~/.config/regnmed/maskinporten-test.env; set +a
```

The production key does not exist yet; it is blocked on the scope grant
(docs/gov.md).

## Handling

- **Anything that does need keeping goes in the password manager**
  (NordPass) — not a synced folder, not the repo, not an encrypted file
  in a synced folder. As it happens the Maskinporten keys do not need
  keeping at all; see "No backup, by design" below.
- **Per machine, not copied.** Maskinporten accepts several keys per
  client, which is what `MASKINPORTEN_KID` selects. A second development
  machine should generate its **own** keypair and register that public
  key on the same client. A private key that never moves cannot be
  intercepted in transit or left behind in a sync folder — and revoking
  one machine then does not disturb the other.

  `scripts/maskinporten-key.sh` does the generating: RSA 2048 in PKCS#8
  (the format `regnmed-gov` is tested against), private key straight to
  `~/.config/regnmed/` at mode 600, public half emitted as a JWK to paste
  into Samarbeidsportalen → the client → **Nøkler** → **Legg til**.

  Generating locally rather than letting Samarbeidsportalen generate the
  pair matters: the private key then never crosses the network and never
  exists anywhere but the machine that uses it.

### Two things the portal decides, not us

- **The `kid` is assigned by Samarbeidsportalen** — a UUID shown as the
  key's heading, e.g. `6e9b86f2-7f8c-4cc2-9451-f3b9b2664742`. The
  RFC 7638 thumbprint the script prints is only a local identifier; the
  UUID is what belongs in `MASKINPORTEN_KID`. Read it back after
  registering.
- **Keys expire.** The portal shows an expiry date per key (the current
  test key: 24.07.2027). An expired key does not degrade gracefully — it
  fails the next token request, which in practice means it fails at a
  frist. Add the date to the December regelverksrevisjon checklist
  (docs/regelverk.md) so it is noticed a year early rather than a day
  late.

### No backup, by design

The private keys are deliberately **not** backed up. Samarbeidsportalen
can add and delete keys on the client at any time with ID-porten login,
so a lost machine is handled by deleting its key there and running the
script on the replacement. A backup would only create another copy of a
secret to protect, in exchange for recovering something that takes two
minutes to recreate.

## If a secret ever does need to be shared through git

It does not, today. If that changes, the tool to reach for is SOPS with
age recipients (encrypts values, leaves keys readable, one public key
per machine) — but adopting it means amending the absolute rule above,
deliberately and in writing. Do not let it happen by accident.
