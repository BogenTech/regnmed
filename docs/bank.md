# Bank reconciliation

Reconciliation proves that the ledger's bank account and the bank's own
records agree — bokføringsforskriften expects bankavstemming as part of
ajourhold, and unexplained differences are the classic audit finding.

## Connectivity tiers (how statements reach regnmed)

| Tier | Mechanism | Status |
| --- | --- | --- |
| 1. File upload | **camt.053** (ISO 20022) exported from any Norwegian nettbank — no bank agreement needed | **Implemented** |
| 1b. CSV upload | Bank CSV exports, layout auto-detected from headers | **Implemented** |
| 2. PSD2 / open banking | Live account feeds via an AISP — requires a Finanstilsynet AISP license or a commercial aggregator (Neonomics, Tink, Mastercard/Aiia) | Later — commercial/licensing decision |
| 3. Direct filutveksling | SFTP/ISO 20022 agreements per bank or via Mastercard Payment Services (also OCR-giro, issue #16) | Later — per-customer onboarding |

All tiers feed the **same** reconciliation engine; only the transport
differs. Building file-first was deliberate: it works for every customer
on day one with zero agreements.

## Data model (migration 0007)

- `bank_statement` — one imported statement per ledger bank account.
  Statements are dokumentasjon: insert-only for the app role, and
  re-import of the same bank statement id (`Stmt/Id`) is rejected —
  imports are idempotent.
- `bank_transaction` — statement lines. Amounts are stored in **ledger
  sign for the bank account**: money in (CRDT) = debit = positive, so a
  bank transaction equals its ledger entry exactly.
- `bank_match` — links one bank transaction to one ledger entry
  (`method` auto/manual, who, when). Unique on both sides.
  **"Unmatched" is always computed as the absence of a match row** —
  never stored mutable state, same philosophy as balances.

## The pieces

- **camt.053 parser** (`regnmed-core::camt053`, pure): tolerant,
  version-agnostic (`camt.053.001.0x`), reads only what reconciliation
  needs (statement id, IBAN, OPBD/CLBD balances, booked entries with
  date/amount/direction/reference/description). Pending (`PDNG`) entries
  are skipped. Amounts parse to integer øre.
- **CSV parser** (`regnmed-core::bankcsv`, pure): tier 1b. Detects the
  layout from the header row instead of maintaining per-bank profiles —
  delimiter (`;`/tab/`,`), date column ("dato"/"bokføringsdato"/…,
  never "rentedato"), one signed beløp column or separate inn/ut
  columns, Norwegian number formats ("1 234,56"), optional
  KID/referanse. A file it cannot understand fails loudly with the
  headers it saw — never a silent half-import. Output is the same
  statement shape as camt.053, so storage, matching and reconciliation
  are one engine; the statement ref is the file's content hash, so
  re-import stays idempotent, and balances are absent (a CSV has none —
  shown as absent, never zero). Same endpoint: the import
  distinguishes XML from CSV by content.
- **Matching engine** (`regnmed-core::bank`, pure, deterministic):
  equal amount + same day first, then equal amount within a ±5-day
  window. Each side is used at most once. **Ties are never guessed** —
  two equally close candidates fall to manual review. KID/reference
  matching arrives with reskontro (entries don't carry payment
  references yet).
- **Web API** (engagement-guarded; revisor 'les' may read, importing and
  matching require bokforing/admin):
  - `POST /companies/{id}/bank/statements?account=1920` — camt.053 XML
    body; imports and auto-matches, returns counts.
  - `GET /companies/{id}/bank/reconciliation?account=1920` — ledger
    balance vs latest statement closing balance, matched count, and both
    unmatched lists.
  - `POST /companies/{id}/bank/matches` + `DELETE
    /companies/{id}/bank/matches/{bank_transaction_id}` — manual
    match/unmatch; cross-company and cross-account pairings are rejected
    in the database layer.

## OCR giro (KID-innbetalinger)

OCR files from Mastercard Payment Services (tidligere Nets/BBS) carry
KID-tagged incoming payments — the detail behind the lump-sum "OCR
innbetaling" line on the bank statement.

- **Parser** (`regnmed-core::ocr`, pure): the official fixed-width
  80-character record format (service 09), layouts verified against the
  published systemspesifikasjon and the netsgiro reference
  implementation. Control records are enforced (transaction count and
  sum must match, so truncated or tampered files are rejected);
  reversal signs are honored; **invalid KIDs are flagged, never
  rejected** — the bank accepted the payment, so it must be recorded.
- **KID check digits** (`regnmed-core::kid`): MOD10 and MOD11
  validation, plus check-digit generation for faktura (issue #13).
- **Storage** (migration 0008): batches insert-only and idempotent per
  (forsendelse, oppdrag) — a re-uploaded file errors instead of
  duplicating payments.
- **Web API**: `POST /companies/{id}/ocr/files?account=1920` (file
  body), `GET /companies/{id}/ocr/payments?from=&to=`. Same access
  rules as bank reconciliation.
- With **reskontro** (issue #3), payments auto-apply to open invoices by
  KID; until then the payment list is the working view, and OCR batch
  sums reconcile against the statement's lump-sum line.

## Where it is tested

- `regnmed-core/src/camt053.rs` — parsing (balances, signs, pending
  skipped, entities, øre), malformed input.
- `regnmed-core/src/bank.rs` — matching rules, window, no-reuse,
  ambiguity-goes-to-manual.
- `regnmed-api/tests/bank.rs` (real Postgres, also CI) — the whole flow
  over HTTP: revisor 403 on import, import + auto-match, duplicate
  import rejected, revisor reads reconciliation, manual match to full
  reconciliation, unmatch, stranger 404.
