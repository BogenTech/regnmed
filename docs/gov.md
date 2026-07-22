# The government rail: Maskinporten and Skatteetaten's APIs

Everything regnmed sends to Norwegian authorities rides on
**Maskinporten** (Digdir's machine-to-machine OAuth2 server). The client
lives in `crates/regnmed-gov` and is shared by every government
integration — mva-melding first, later skattemelding and årsregnskap.

## Maskinporten (crates/regnmed-gov/src/maskinporten.rs)

Flow (RFC 7523): sign a short-lived JWT grant with the key registered on
our Maskinporten client → POST to the token endpoint → receive an access
token carrying the requested scopes → cache until shortly before expiry.
Grant constraints honored: `exp − iat ≤ 120 s`, unique `jti`, `aud` =
the Maskinporten issuer.

Configuration (environment):

| Variable | Example |
| --- | --- |
| `MASKINPORTEN_TOKEN_ENDPOINT` | `https://test.maskinporten.no/token` |
| `MASKINPORTEN_AUDIENCE` | `https://test.maskinporten.no/` |
| `MASKINPORTEN_CLIENT_ID` | client UUID from Samarbeidsportalen |
| `MASKINPORTEN_KEY_FILE` | path to the RS256 private key (PEM) |
| `MASKINPORTEN_KID` | key id, if several keys are registered |
| `MASKINPORTEN_SCOPES` | `skatteetaten:mvameldingvalidering` |

Production uses `https://maskinporten.no`. **No secrets in the repo,
ever** — keys live outside git and outside container images.

### Operational setup (one-time, per environment)

1. Get access to Digdir's **Samarbeidsportalen** (requires the
   organization's Altinn roles).
2. Create a Maskinporten client in the test environment; register a
   public key (or virksomhetssertifikat) on it.
3. Request the Skatteetaten scopes (`skatteetaten:mvameldingvalidering`,
   later `skatteetaten:mvamelding` for submission). Skatteetaten grants
   them to the org.
4. Point the env variables above at the test environment and run
   `regnmed mva-melding … --validate`.

For a regnskapsfører acting on behalf of clients, the client company
**delegates** the scope to the accounting firm in Altinn (Altinn
Autorisasjon). This maps 1:1 onto regnmed's engagement model — the same
firm→client relationship, expressed in the government's registry.

## Mva-melding APIs (crates/regnmed-gov/src/mvamelding.rs)

- **Validation** (implemented): POST the melding XML to Skatteetaten's
  validation endpoint (test:
  `https://idporten-api-sbstest.sits.no/api/mva/grensesnittstoette/mva-melding/valider`,
  override with `MVA_VALIDATION_URL`). The returned
  `valideringsresultat` is kept verbatim — it is documentation of a
  control — and any avvik fail the CLI run.
- **Submission** (pending): the Altinn3 instance flow (create instance →
  upload melding + konvolutt → confirm → poll feedback). Deliberately
  not implemented until we hold real test credentials, so it can be
  developed against the actual test environment instead of guessed.
  Tracked in issue #8.

## Status & verification

- Implemented and locked by tests (no credentials needed): grant-JWT
  claims (decoded and checked against the spec), the full token flow and
  its cache against a local mock endpoint, melding XSD validity.
- Requires real test credentials (next step, blocked on client
  registration): live validation round-trip, delegation, submission.
