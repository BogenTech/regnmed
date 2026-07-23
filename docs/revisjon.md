# Revisorrollen og verifikasjonsrapporten

The pitch to revisorer is the product's core promise turned into a
workflow: **don't trust us — verify**. This document covers what the
revisor role can do and what the verification report states.

## The role

A revisjonsfirma reaches a client through the same marketplace flow as
regnskapsførere ([marketplace.md](marketplace.md)): verified
autorisasjon (Finanstilsynet, fail-closed), request → accept →
engagement. An engagement of kind `revisjon` resolves to **`les`
access** — the revisor can read everything (reports, reskontro, bilag,
attachments, anchors) and mutate nothing; every write endpoint requires
`bokforing` or `admin`. Ending the engagement revokes access
immediately (valid_to is exclusive — [auth.md](auth.md)).

## The verification report

`GET /companies/{id}/reports/revisjon` (portal: Rapporter → Revisjon)
runs every check the system can make about its own ledger and states
the outcome. A failed check becomes an **AVVIK** line in the report —
it is never an error that hides the document.

| Kontroll | What it proves |
| --- | --- |
| Hash-kjede fra genesis | every voucher re-hashed from stored content; links and chain head intact ([ledger.md](ledger.md)) |
| Bilagsvedlegg | attachment bytes re-hashed against stored SHA-256 ([perioder.md](perioder.md)) |
| Ekstern forankring | anchored heads still on the live chain; stored roots recompute from their leaves ([anchoring.md](anchoring.md)) |
| Reskontro mot hovedbok | every posting on a reskontro-flagged account carries a party — subledger equals hovedbok by construction |
| Balansekontroll | all postings sum to exactly zero øre |
| Periodelåsing | current lock and the size of the insert-only lock history (informational) |
| Regelverkssatser | no monitored sats domain in the satsregister is older than its known change cadence ([regelverk.md](regelverk.md)) — outdated satser would silently produce unlawful gebyrer/renter |

The report also lists every external anchor covering the company
(timestamp, sequence, root hash, witnesses) and the chain head at
generation time.

`?format=tekst` downloads a **deterministic plain-text rendering**
(`regnmed-core::revisjon::render_text` — same input, same bytes) meant
for the revisor's own archive, ending with the independent
re-verification procedure: re-walk the chain from the documented
format, compare roots against the public `/anchors` feed and one's own
copies, verify RFC 3161 tokens offline with `openssl ts`.

Any access level may generate the report — verification never mutates —
and no access yields 404, as everywhere.

## Where it is tested

- `crates/regnmed-core/src/revisjon.rs` — deterministic rendering, the
  verdict flip on a failed kontroll, "no anchors" stated not hidden.
- `crates/regnmed-api/tests/revisjon.rs` (real Postgres, also in CI) —
  a revisor whose only path is a `revisjon` engagement generates the
  report; all six kontroller pass on a healthy ledger (reskontro,
  period lock and anchor present); a planted anchor mismatch flips
  `alle_ok` and marks Ekstern forankring AVVIK; the text download
  renders with the verdict; outsiders get 404.
