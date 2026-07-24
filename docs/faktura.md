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

## Salgsdokumentet som PDF (#32)

- **Deterministic, hand-rolled renderer** (`regnmed-core::pdf` +
  `fakturapdf`): the three standard PDF fonts (no embedding),
  WinAnsi/CP1252 for æøå og typografi, ~3 KB per invoice, no rendering
  engine (frugality). Same input → byte-identical output forever.
- **Stored at issue time, in the issuing transaction**, as an
  attachment on the invoice's voucher — the document the customer
  receives is part of oppbevaringen from the moment the invoice exists,
  hash-checked on every download like all dokumentasjon. Serving is a
  DB read; nothing renders on the request path.
- Contents per bokføringsforskriften §5-1-1: nummer/dato, selger med
  orgnr ("MVA" when the document carries VAT, "Foretaksregisteret" for
  AS/ASA), kjøper, linjer, mva spesifisert i NOK per sats, forfall,
  KID og kontonummer.
- **Kontaktinfo** (migration 0019, editable master data, never hashed):
  company address/kontonummer/selskapsform via
  `GET/PUT /companies/{id}/settings` (PUT is admin-only); party
  address/e-mail via `PUT …/parties/{pid}/contact`. Portal:
  Firmaopplysninger card on Oversikt, Kontaktinfo on the party page.
- Purringer/inkassovarsler render their stored text deterministically
  to PDF on demand (`?format=pdf`, docs/purring.md).

## E-postutsendelse (#32)

- **One rail for all outbound mail**: regnmed publishes to the same
  JetStream stream regnid's mail workers consume (`REGNID_MAIL` /
  `regnid.mail.send` — a wire contract; regnid's `OutboundMail` gained
  serde-defaulted `reply_to` + base64 `attachments` for it). SMTP/Brevo
  stay configured in exactly one place, the worker.
- **Sending is an explicit human action** (portal Send buttons, or
  `POST …/invoices/{iid}/send` / `POST …/reminders/{rid}/send`, both
  bokforing+). Recipient defaults to the party's stored e-mail,
  overridable per send; **reply-to is the company's own address**
  (settings), never regnmed's.
- **Insert-only utsendelseslogg** (migration 0020): who sent what to
  whom, when. The log id doubles as the queue's `Nats-Msg-Id`, so a
  retried publish deduplicates in the stream — the log row and the
  queue message are the same event. `GET …/invoices/{iid}/utsendelser`.
- The attached PDF is the stored salgsdokument (hash-checked on read) —
  the mail carries byte-exactly what oppbevaringen holds.
- Unconfigured rail (no `NATS_URL`) → the endpoints answer with a clear
  message instead of pretending.

## Not yet (deliberate)

- **EHF dispatch** arrives with the Peppol access point (issue #14).

Purring, forsinkelsesrente og inkassovarsel: shipped — docs/purring.md.

## Web API (mutations require bokforing)

| Endpoint | Purpose |
| --- | --- |
| `POST /companies/{id}/invoices` | issue (party_no, dates, lines; defaults: journal GL, receivable 1500, VAT 2700, account 3000, quantity 1) |
| `GET /companies/{id}/invoices?open=true` | list with reskontro remaining per invoice |
| `POST /companies/{id}/invoices/{iid}/credit-note` | full kreditnota |
| `GET /companies/{id}/invoices/{iid}/pdf` | the stored salgsdokument (hash-checked) |
| `GET/PUT /companies/{id}/settings` | firmaopplysninger for the PDF (PUT admin-only) |
| `PUT /companies/{id}/parties/{pid}/contact` | party address/e-mail |

## Where it is tested

- `regnmed-core/src/invoice.rs` — line/VAT computation, rounding, KID
  validity, voucher balance with party, credit-note sign flip.
- `regnmed-api/tests/invoice.rs` (real Postgres, also CI) — the whole
  loop over HTTP: issue (12 500 kr, valid KID), failed attempt burns no
  number, chain verifies, OCR payment resolves the invoice by KID,
  kreditnota auto-settles, double-credit rejected.
- `regnmed-core/src/pdf.rs` + `fakturapdf.rs` — valid xref structure,
  WinAnsi encoding incl. CP1252 typografi, escaping, width-based right
  alignment, determinism, lovpålagt innhold, kreditnota variant,
  pagination.
- `regnmed-api/tests/faktura_pdf.rs` (real Postgres, also CI) —
  settings over the API, the PDF exists as a verified attachment the
  moment the invoice does, served with the kontaktinfo, kreditnota
  document, purring `?format=pdf`, settings PUT rejected for
  non-admins.
- `regnmed-api/tests/utsendelse.rs` (real Postgres + a spawned
  `nats-server`, skips without either) — the send endpoint puts a real
  JetStream message on the rail in regnid's wire format (attachment
  base64-decodes back to the stored PDF, reply-to = company),
  the log records it, and an unconfigured rail answers clearly.
  regnid's own suite pins the wire format's backward compatibility.
