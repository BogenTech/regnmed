# Reskontro: kunde- og leverandørspesifikasjon

Bokføringsforskriften §3-1 requires subledgers per customer and supplier.
In regnmed a **party** (kunde/leverandør) is master data with a numeric
party number; ledger entries on reskontro-flagged accounts carry the
party, and the spesifikasjon and åpne poster are pure queries over those
entries.

## The party binding is tamper-evident (hash format v2)

The party number is **inside the voucher hash**. Reassigning a
receivable from one customer to another — the classic subledger fraud —
breaks the chain like any other edit. This required the first hash
format version bump, done as designed (docs/ledger.md):

- Every voucher stores `hash_version`. Existing history is v1 and
  verifies unchanged forever; new postings are v2 (a `"v2"` marker field
  plus the party number per entry, empty when none).
- Mixed-version chains verify from genesis; both formats are frozen and
  pinned by golden tests.

## Rules (enforced at posting, in one place)

- An account flagged `reskontro_kind` ('kunde'/'leverandor') **requires**
  a party of that kind on every entry.
- Ordinary accounts **reject** parties.
- Party numbers are immutable business identifiers (they are hashed);
  names/orgnr stay editable master data. Auto-numbering: kunder from
  10000, leverandører from 50000.

## Åpne poster

`reskontro_match` links an invoice-side entry to a settlement-side entry
for an amount — partial settlements are rows, an item's *remaining* is
always computed (`amount − matched`), never stored. Over-matching a
remainder is rejected; matches are per party and account, cross-company
pairings impossible.

## Web API (engagement-guarded; mutations require bokforing)

| Endpoint | Purpose |
| --- | --- |
| `GET /companies/{id}/parties?kind=kunde` | spesifikasjon: parties with saldo |
| `POST /companies/{id}/parties` | create party (auto party_no) |
| `GET /companies/{id}/parties/{pid}/items?open=true` | items with matched/remaining |
| `POST /companies/{id}/reskontro/matches` + `DELETE …/{match_id}` | åpne poster matching |
| `PUT /companies/{id}/accounts/{nr}/reskontro` | flag/clear a reskontro account |

## SAF-T

The export now includes `Customers`/`Suppliers` master data with
subledger opening/closing balances, and invoice/payment lines carry
`CustomerID`/`SupplierID` — validated against the official XSD like the
rest of the file.

## What this unlocks next

- **Faktura (#13)**: invoices post against parties with KID; OCR
  payments (docs/bank.md) then auto-apply by KID.
- Purring/aldersfordelt saldoliste are queries over åpne poster.

## Where it is tested

- `regnmed-core/src/hash.rs` — golden digests for v1 **and** v2;
  party reassignment changes the v2 hash; v1/v2 never collide.
- `regnmed-api/tests/reskontro.rs` (real Postgres, also CI) — the whole
  flow over HTTP: flag account, create customer, party-requirement
  enforcement both ways, invoice + partial payment, mixed-version chain
  verification, spesifikasjon saldo, åpne poster matching with
  over-match rejection, SAF-T with customers.
