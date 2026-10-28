# Secrets

**Nothing secret lives in this repository, in any form — not even
encrypted.** docs/gov.md's "No secrets in the repo, ever" is absolute,
for test as well as production, and there is no carve-out.

That is affordable because regnmed barely has any secrets. What looks
like configuration mostly is configuration:

| What | Where | Secret? |
| --- | --- | --- |
| `DATABASE_URL`, `OIDC_ISSUER`, `OIDC_AUDIENCE`, `BIND_ADDR` | `.env`, from `.env.example` | **No.** Localhost, throwaway dev password. Committed as an example on purpose. |
| Maskinporten test key + client id | `~/.config/regnmed/` (see below) | **Yes.** |
| `regnmed-agent` client secret (maskinaktør i prod) | `~/.config/regnmed/` (see below) | **Yes.** |
| Production database credentials | Created in the cluster, out-of-band | **Yes.** `deploy/prod` is built so no credential appears in any rendered manifest (docs/deploy.md). |

A new machine gets a working dev environment with
`cp .env.example .env` and `docker compose up -d`. Nothing needs to be
carried across.

## Where the real secrets are

A secret correctly kept out of the repo is a secret nobody can find
again. Recording the *location* is not recording the secret:

```
~/.config/regnmed/maskinporten-test.pem       RSA 2048 private key, mode 600
~/.config/regnmed/maskinporten-test.pub.pem   public half (registered in Samarbeidsportalen)
~/.config/regnmed/maskinporten-test.env       MASKINPORTEN_* config incl. client id og kid
```

Generated 2026-07-24 alongside the Samarbeidsportalen setup. `MASKINPORTEN_KID`
er UUID-en Samarbeidsportalen tildelte nøkkelen — uten den svarer
Maskinporten «No key identifier (kid) provided», og det ser ut som en
nøkkelfeil framfor en manglende innstilling. Nothing in
the repo references these paths — `regnmed-gov` reads
`MASKINPORTEN_KEY_FILE` and friends from the environment, so you source
the env file when you need them:

```bash
set -a; . ~/.config/regnmed/maskinporten-test.env; set +a
```

The production key does not exist yet; it is blocked on the scope grant
(docs/gov.md).

### regnmed-agent (maskintilgang til prod-API-et)

En `client_credentials`-klient i PROD-regnid, registrert 2026-10-28 for
å prøve ut agent-tilgang (#101–#103). Den er en integrasjon som alle
andre: tilgangen ligger i regnmeds base, ikke i tokenet, og trekkes
tilbake i portalen under Administrasjon → Oppdrag og integrasjoner.

```
~/.config/regnmed/regnmed-agent.env   client id + secret, mode 600
```

**regnid lagrer bare hashen av hemmeligheten.** Filen over er den
eneste kopien — mistes den, finnes ingen gjenoppretting, bare en ny
klient. Ingen backup, av samme grunn som for Maskinporten-nøkkelen: en
ny maskin lager sin egen klient, og en tapt maskin håndteres ved å
trekke tilbake tilgangen i portalen og slette klienten i regnid.

```bash
set -a; . ~/.config/regnmed/regnmed-agent.env; set +a
curl -s -X POST "$REGNMED_AGENT_ISSUER/token" \
  -d grant_type=client_credentials \
  -d client_id="$REGNMED_AGENT_CLIENT_ID" \
  --data-urlencode client_secret="$REGNMED_AGENT_CLIENT_SECRET"
```

Merk: regnids token-endepunkt krever `client_id`/`client_secret` i
KROPPEN. HTTP Basic (`client_secret_basic`) svarer 422 «missing field
client_id» — et avvik fra RFC 6749 §2.3.1, som sier at en server MÅ
støtte Basic for klienter som har fått en hemmelighet. Verdt å rette i
regnid før OIDC-konformanstesten, siden et hvilket som helst
standardbibliotek prøver Basic først.

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
