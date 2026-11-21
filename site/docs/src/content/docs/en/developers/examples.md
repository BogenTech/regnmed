---
title: Examples
description: Ready-made curl recipes for the most common flows, built from the actual request structs and tests.
sidebar:
  order: 6
---

All examples assume

```sh
API=https://api.regnmed.no
COMPANY=<the company's id from /me>
TOKEN=<access_token, see "Getting started">
AUTH="Authorization: Bearer $TOKEN"
```

Amounts are written in øre: NOK 1,250.00 is `125000`. Dates are `YYYY-MM-DD`. The required right for each endpoint is in the [API reference](/en/developers/api/).

## 1. Which companies can I work for?

```sh
curl -s $API/me -H "$AUTH" | jq '.companies[] | {company_id, name, access, rettigheter}'
```

Call this first in a session. `rettigheter` (rights) tells you what you are allowed to do before you try something that gets refused.

## 2. Trial balance for a period

```sh
curl -s "$API/companies/$COMPANY/reports/saldobalanse?from=2026-01-01&to=2026-12-31" -H "$AUTH"
```

```json
{ "accounts": [
  { "number": "1920", "name": "Bankinnskudd",
    "inngaende_ore": 0, "debet_ore": 12500000, "kredit_ore": 4200000, "utgaende_ore": 8300000 }
] }
```

(`inngaende_ore` is the opening balance, `debet_ore`/`kredit_ore` the period's debit and credit, `utgaende_ore` the closing balance.)

Income statement and balance sheet have their own endpoints: `…/reports/resultat?from=&to=` and `…/reports/balanse?date=`. Account ledger for a single account: `…/reports/kontospesifikasjon?from=&to=&account=1920`.

## 3. Post a manual voucher

The lines must sum to zero (debit positive, credit negative). The required right is `BILAG_BOKFOR` (post voucher).

```sh
curl -s -X POST $API/companies/$COMPANY/vouchers -H "$AUTH" -H 'Content-Type: application/json' -d '{
  "journal_code": "GL",
  "date": "2026-03-01",
  "description": "Overføring til skattetrekk",
  "lines": [
    { "account": "1921", "amount_ore": 500000 },
    { "account": "1920", "amount_ore": -500000 }
  ]
}'
```

```json
{ "voucher_id": "0192f…", "voucher": "2026-14", "chain_seq": 14 }
```

A line can also carry `vat_code`, `party_no`, `description`, `avdeling` (department) and `prosjekt` (project). The voucher is rejected with `400` and an explanatory message on imbalance, an unknown or deactivated account, a locked period, or a VAT code the company cannot use. A voucher is never edited. You reverse it:

```sh
curl -s -X POST $API/companies/$COMPANY/vouchers/$VOUCHER_ID/reverse -H "$AUTH" \
  -H 'Content-Type: application/json' -d '{ "dato": "2026-03-02" }'
```

(`dato` is the reversal date.)

## 4. Create a customer and an invoice

```sh
curl -s -X POST $API/companies/$COMPANY/parties -H "$AUTH" -H 'Content-Type: application/json' -d '{
  "kind": "kunde", "name": "Kunde AS",
  "email": "regnskap@kunde.example", "address": "Storgata 1, 0155 Oslo"
}'
# { "party_id": "…", "party_no": "10001" }

curl -s -X POST $API/companies/$COMPANY/invoices -H "$AUTH" -H 'Content-Type: application/json' -d '{
  "party_no": "10001",
  "invoice_date": "2026-02-01",
  "due_date": "2026-02-15",
  "lines": [{
    "description": "Konsulentbistand",
    "quantity_milli": 2500,
    "unit_price_ore": 400000,
    "vat_code": "3"
  }]
}'
```

(`kind: "kunde"` means customer.)

```json
{
  "invoice_id": "…", "invoice_no": 1001, "kid": "…",
  "net_ore": 1000000, "vat_ore": 250000, "gross_ore": 1250000,
  "gross_nok_ore": 1250000, "voucher": "2026-15"
}
```

The invoice number and KID are assigned atomically with the voucher. The VAT rate is looked up from the **invoice date**. `vat_code` `3` is output VAT at 25%. Defaults: journal `GL`, accounts receivable `1500`, VAT account `2700`, revenue account `3000` (or the product's). Optional fields: `leveringsdato` (delivery date), `leveringssted` (delivery place), `valuta` (currency), `produkt` (product) per line, `timer_entry_ids` to invoice unbilled hours, and `buyer_reference`.

The invoice is **immutable**. Mistakes are corrected with a credit note:

```sh
curl -s -X POST $API/companies/$COMPANY/invoices/$INVOICE_ID/credit-note -H "$AUTH"
```

PDF and EHF are fetched with `GET …/invoices/{id}/pdf` and `GET …/invoices/{id}/ehf`.

## 5. Send the invoice

```sh
curl -s -X POST $API/companies/$COMPANY/invoices/$INVOICE_ID/send -H "$AUTH" \
  -H 'Content-Type: application/json' -d '{ "email": "annen@kunde.example" }'
```

```json
{ "job_id": "…", "status": "queued" }
```

The response is `202`: a worker chooses EHF or email and sends it. Fetch the outcome with `GET $API/companies/$COMPANY/jobs/$JOB_ID`. When `status` is `done`, `result` says which `kanal` (channel) was used.

The body is optional. Without `email`, the customer's stored address is used. The required right is `FAKTURA_SEND`. Without a configured mail rail (`NATS_URL`) you get `400` with a clear message instead of the dispatch pretending it went out.

## 6. Upload a document to the inbox

The file is sent as the raw body, with the filename and SHA-256 in the query:

```sh
FILE=kvittering.pdf
SHA=$(shasum -a 256 "$FILE" | cut -d' ' -f1)

curl -s -X POST "$API/companies/$COMPANY/inbox?filename=$FILE&sha256=$SHA" \
  -H "$AUTH" -H 'Content-Type: application/pdf' --data-binary @"$FILE"
# { "document_id": "…", "existing": false }
```

If you run the same call again, you get `existing: true` and no duplicate. The required right is `BILAG_LAST_OPP` (upload voucher).

This is how the flow continues. The suggestion is **a suggestion and never a posting**:

```sh
# regnmed's own suggestion, with the reasoning per field
curl -s $API/companies/$COMPANY/inbox/$DOC_ID/forslag -H "$AUTH"

# post the document (the lines must sum to zero)
curl -s -X POST $API/companies/$COMPANY/inbox/$DOC_ID/bokfor -H "$AUTH" \
  -H 'Content-Type: application/json' -d '{
  "journal_code": "GL", "date": "2026-03-05", "description": "Kontorrekvisita",
  "lines": [
    { "account": "6800", "amount_ore": 80000, "vat_code": "1" },
    { "account": "2710", "amount_ore": 20000 },
    { "account": "2400", "amount_ore": -100000, "party_no": "20001" }
  ],
  "due_date": "2026-03-20", "kid": "1234567"
}'

# or reject, with a reason that stays on record
curl -s -X POST $API/companies/$COMPANY/inbox/$DOC_ID/avvis -H "$AUTH" \
  -H 'Content-Type: application/json' -d '{ "note": "Does not belong in these books" }'
```

(`forslag` = suggestion, `bokfor` = post, `avvis` = reject.)

With an active approval policy (attestering), a human must approve before `bokfor` succeeds, and the same person cannot both approve and post.

## 7. Export SAF-T

```sh
curl -s -OJ "$API/companies/$COMPANY/reports/saft?year=2026" -H "$AUTH"
# or an arbitrary interval: ?from=2026-01-01&to=2026-06-30
```

The response is an XML file (`SAF-T Financial_<orgnr>_<timestamp>.xml`), validated against the Norwegian Tax Administration's XSD. The contact person in the header is taken from the token's name. For a machine without a full name, send `contact_first=` and `contact_last=`.

## 8. VAT specification for a period

```sh
curl -s "$API/companies/$COMPANY/reports/mva?year=2026&termin=2" -H "$AUTH"
```

```json
{
  "year": 2026, "termin": 2, "ordning": "to-maneder", "frist": "2026-06-10",
  "lines": [{ "code": "3", "description": "…", "rate_bp": 2500,
              "grunnlag_ore": 1000000, "avgift_ore": 250000, "virkning_ore": 250000,
              "retning": "utgaende" }],
  "utgaende_ore": 250000, "inngaende_ore": 0, "netto_ore": 250000
}
```

(`termin` is the VAT period number, `ordning` the period scheme, `frist` the deadline, `grunnlag_ore` the tax base, `avgift_ore` the VAT amount, `utgaende_ore`/`inngaende_ore` output/input VAT, `netto_ore` the net payable.)

The period number follows the company's period scheme (`GET …/mva/terminordning`). A number outside the scheme gives `400`. The postings behind one line: `…/reports/mva/postings?year=&termin=&kode=3`. The VAT return itself: `…/reports/mva-melding`.

## 9. Open items and customer subledger

```sh
# customers and their balance
curl -s "$API/companies/$COMPANY/parties?kind=kunde&sort=overdue&desc=true&limit=10" -H "$AUTH"

# open items for one customer
curl -s "$API/companies/$COMPANY/parties/$PARTY_ID/items?open=true" -H "$AUTH"
```

```json
{ "items": [{ "entry_id": "…", "voucher": "2026-15", "date": "2026-02-01",
              "due_date": "2026-02-15", "kid": "…",
              "amount_ore": 1250000, "matched_ore": 0, "remaining_ore": 1250000 }] }
```

Overdue invoices with age and the suggested next reminder step: `GET …/invoices/overdue`. Combined customer and supplier ledger specification: `…/reports/kundespesifikasjon` and `…/reports/leverandorspesifikasjon`.

## 10. Verify the general ledger

```sh
curl -s $API/companies/$COMPANY/anchors/verify -H "$AUTH"
# { "ok": true, "vouchers_checked": 1523, "attachments_checked": 311, "anchors_checked": 42, "problems": [] }
```

See [Verify it yourself](/en/developers/verify-it-yourself/). The audit report with eight checks: `GET …/reports/revisjon` (add `?format=tekst` for a downloadable text version).
