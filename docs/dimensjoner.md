# Dimensjoner: avdeling og prosjekt

Cost/revenue follow-up per avdeling and prosjekt, as first-class,
hash-covered data on entry lines. Foundation for timeføring (#38),
budsjett (#41) and prosjektrapporter.

## The registry

`dimension` (migration 0018): per company, `kind` (avdeling|prosjekt),
`code`, `name`, `active`. Lifecycle is **insert + rename + open/close
only**, enforced by trigger and column grants:

- The **code is permanent** — it is inside the v3 voucher hash of every
  posting that references it; changing it would break chain
  verification. The trigger rejects identity changes for every role.
- The **name** may change (it is not hashed) and a rename does not
  disturb verification — tested.
- **Avsluttet** (`active = false`) rejects *new* postings, exactly like
  a locked period: corrections happen via open dimensions. History on a
  closed dimension keeps reporting and verifying forever. Reopening is
  one update — no data is ever deleted.

## Hash format v3

Entries carry optional `avdeling_id`/`prosjekt_id`; the voucher hash
covers the **codes** (marker `"v3"`, then per line: … party, avdeling,
prosjekt — empty when none). v1/v2 history verifies unchanged forever;
golden tests pin all three digests (docs/ledger.md). Verification
re-reads the codes via join, exactly as it does party numbers.

## Posting validation

`post_voucher` resolves each referenced dimension up front: it must
exist for the company, be of the right kind, and be **active** — a
typo or an avsluttet prosjekt fails the whole voucher with a clear
message before anything is written.

## Reports

- **Resultat per dimension**: `GET …/reports/resultat?from=&to=` takes
  optional `avdeling=` / `prosjekt=` codes — the same pure SUM query,
  filtered. (Balanse is not filtered: dimensions are optional per line,
  so a per-dimension balanse would not balance.)
- **Kontospesifikasjon** rows carry the line's avdeling/prosjekt.
- **SAF-T**: the registry exports as `AnalysisTypeTable` (type codes
  `AVD`/`PRO`, closed dimensions marked `Status=Closed`) and each line's
  codes as `Analysis` elements with the amount on the line's side —
  validated against Skatteetatens XSD in tests and CI.

## Web API (mutations require bokforing)

| Endpoint | Purpose |
| --- | --- |
| `GET /companies/{id}/dimensions` | the registry |
| `POST /companies/{id}/dimensions` | create (kind, code, name) |
| `PUT /companies/{id}/dimensions/{kind}/{code}` | rename and/or open/close |

Lines accept `avdeling`/`prosjekt` codes on: innboks bokføring
(`…/inbox/{id}/bokfor`) and faktura revenue lines
(`POST …/invoices`; the codes are stored on `invoice_line`, so a
kreditnota reverses revenue on the same dimensions).

Portal: registry management in the Bilag section; pickers in the
innboks bokfør form and the faktura form; resultat filter in Rapporter.

## Deliberately not (yet)

- Mandatory dimensions per account (policy config, later).
- More than two dimension kinds.
- Dimension import from SAF-T files (the importer ignores Analysis
  elements for now).

## Where it is tested

- `regnmed-core/src/hash.rs` — golden digests v1/v2/v3; moving a
  dimension code or swapping avdeling↔prosjekt changes the v3 hash;
  v2/v3 never collide.
- `regnmed-core/src/saft.rs` — AnalysisTypeTable + Analysis rendering,
  XSD-validated.
- `regnmed-api/tests/dimensions.rs` (real Postgres, also CI) — registry
  over HTTP (duplicate/bad kind rejected), v3 postings mixed with
  dimension-free vouchers verify from genesis, unknown/avsluttet codes
  rejected at posting, resultat filters sum correctly per dimension,
  kontospesifikasjon carries the codes, SAF-T export contains the
  Analysis elements, the code column is immutable while rename is not.
