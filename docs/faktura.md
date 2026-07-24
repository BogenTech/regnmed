# Utgående faktura

Salgsdokument per bokføringsforskriften §5-1: gap-free invoice numbers,
KID, and automatic posting to ledger + reskontro. Invoices are
**immutable once issued** (insert-only for the app role) — a mistake is
corrected with a kreditnota, never an edit, mirroring the ledger's
reversing-voucher rule.

## Guarantees

- **Fortløpende nummerering**: the invoice number comes from a counter
  bumped in the *same transaction* as the ledger posting
  (`post_voucher_in`), so a failed issue rolls back both — no gaps in
  invoice numbers or voucher numbers, ever. Tested: a rejected invoice
  attempt does not burn a number.
- **KID**: derived from the invoice number (8 digits + MOD10 check,
  `regnmed-core::invoice::invoice_kid`), unique per company. OCR
  innbetalinger resolve their invoice by KID at import and the payment
  list shows which invoice each payment settles (auto-*posting* of
  payments is a later, opt-in step — the bank statement is the posting
  source until then, avoiding double-posting).
- **Posting**: debit receivable (with the customer — hash v2 covers the
  party), credit each revenue line with its VAT code, credit summed VAT.
  Line VAT uses the dated rate valid on the invoice date. Amounts:
  integer øre, `quantity_milli × unit_price_ore / 1000` rounded half
  away from zero.
- **Kreditnota**: same lines negated (signs flow through the whole
  computation), `credits_invoice_id` links the pair, and the two
  receivable entries are auto-matched in reskontro for whatever remained
  open. Double-crediting is rejected.

## Not yet (deliberate)

- Document rendering (PDF/print) and **EHF dispatch** arrive with the
  portal UI and the Peppol access point (issue #14) — the invoice *data*
  is complete and audit-ready now.

Purring, forsinkelsesrente og inkassovarsel: shipped — docs/purring.md.

## Web API (mutations require bokforing)

| Endpoint | Purpose |
| --- | --- |
| `POST /companies/{id}/invoices` | issue (party_no, dates, lines; defaults: journal GL, receivable 1500, VAT 2700, account 3000, quantity 1) |
| `GET /companies/{id}/invoices?open=true` | list with reskontro remaining per invoice |
| `POST /companies/{id}/invoices/{iid}/credit-note` | full kreditnota |

## Where it is tested

- `regnmed-core/src/invoice.rs` — line/VAT computation, rounding, KID
  validity, voucher balance with party, credit-note sign flip.
- `regnmed-api/tests/invoice.rs` (real Postgres, also CI) — the whole
  loop over HTTP: issue (12 500 kr, valid KID), failed attempt burns no
  number, chain verifies, OCR payment resolves the invoice by KID,
  kreditnota auto-settles, double-credit rejected.
