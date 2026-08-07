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
  orgnr (påtegningene under), kjøper, **leveringstidspunkt (og -sted)**,
  linjer, mva spesifisert i NOK per sats, forfall, KID og kontonummer.
  **Begge parters adresse er påkrevd** (§5-1-2) — utstedelsen nekter
  uten dem, siden fakturaen er uforanderlig i det den finnes og et
  manglende felt bare kan rettes ved å kreditere og gjøre om.

### Registreringsstatus (§5-1-2, #81)

"MVA" etter orgnr og påtegningen "Foretaksregisteret" hører til
REGISTRERINGEN, ikke til dokumentet. De ble utledet — `vat_ore != 0`
respektive `orgform in (AS, ASA)` — og begge utledningene er gale i
vanlige tilfeller: en registrert selger som fakturerer eksport eller
fritatt omsetning fikk ingen "MVA", og ANS/DA, NUF og næringsdrivende
ENK er registreringspliktige i Foretaksregisteret uten å være AS.

Statusen er nå lagret i `company_registrering` (migration 0056),
**datert og innsettings-bar** etter mønsteret fra sats og vat_rate:
oppslag = nyeste rad med `valid_from <= dokumentets dato`. Dateringen
er ikke teoretisk — tilbud og ordrebekreftelser rendres *on demand*, så
uten den ville et gammelt tilbud fått dagens status neste gang noen
lastet det ned.

- **Onboarding** henter begge flaggene og forretningsadressen fra
  Enhetsregisteret (`kilde='brreg'`).
- **Korrigering**: `POST …/settings/registrering` (admin) skriver en ny
  datert rad; den forrige blir stående som det som gjaldt da.
- **Eksisterende selskaper** ble backfillet med den gamle utledningen
  løftet fra per-dokument til per-selskap, merket `kilde='migrert'` —
  en ANTAKELSE, og portalen ber om at den bekreftes.
- Ingen rad = ingen påtegning: vi hevder ikke en registrering vi ikke
  har belegg for.

Det samme oppslaget styrer `cac:PartyTaxScheme` i EHF-en.

### Leveringstidspunkt (§5-1-1 nr. 4, #81)

The forskrift requires *when* the ytelse was delivered on every
salgsdokument, and it is **not** the same fact as the invoice date —
anything billed in arrears separates them. `InvoiceDraft.delivery_date`
is therefore not an `Option`: every caller has to decide. The choices
in the tree today:

| Path | Leveringsdato |
| --- | --- |
| `POST …/invoices` | `leveringsdato` in the request; omitted = the invoice date (the default is declared at the boundary, and the portal always sends it) |
| Timer → faktura | the **last day worked** among the billed hours — that is when the ytelse was complete, often weeks before billing |
| Kombinert faktura (varer + timer) | the caller's date, unless hours were worked later; then the later one wins, so the document cannot claim delivery before the work happened |
| Ordre → faktura | `leveringsdato` on the request; omitted = the invoice date |
| Repeterende faktura | the period's own start date (`neste_dato`) |
| Kreditnota | the **original** invoice's delivery date — it credits that delivery; dating it today would assert a delivery that never happened |
| Tilbud / ordrebekreftelse | none: nothing is delivered yet, and these are not salgsdokumenter |

The database column is **nullable, deliberately**: invoices issued
before #81 have no recorded delivery date, and backfilling them with
the invoice date would invent a legal fact on an immutable document.
That history stays visibly incomplete; the requirement is enforced at
issue time instead. In EHF the field is `cac:Delivery/cbc:ActualDeliveryDate`
(BT-72) — omitted entirely rather than guessed when absent.
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

## Repeterende faktura (#30)

- **A template is a plan, not evidence**: customer + lines + intervall
  (månedlig/kvartalsvis/årlig) + neste/slutt-dato, ordinary editable
  data. Nothing regnskapsmessig exists until generation.
- **Generation adds no posting semantics**: it issues a NORMAL invoice
  through the existing path — gap-free number, KID, posting, PDF, all
  in one transaction — with `{måned}`/`{år}` in line texts interpolated
  to the generated period ("Husleie august 2026"). Month arithmetic
  clamps into shorter months (31. jan + 1 mnd = 28. feb).
- **Idempotent and atomic**: the template row is locked, the invoice +
  run-log row + neste_dato advance commit together, and a partial
  unique index makes a period impossible to generate twice — even
  under concurrent runs. Failures roll back, log a failure row
  (insert-only log), and leave neste_dato untouched for retry. A
  template several periods behind catches up.
- **Driven by the daily CronJob** (`regnmed generate-invoices`,
  deploy/base — same pattern as anchoring); the portal's "Generer nå"
  is the same code path. `merk_utsendelse` MARKS the generated invoice
  for sending in the run log — the send itself stays a human click.
- Templates are deactivated, never deleted, once they have runs (FK
  enforces it).

| Endpoint | Purpose |
| --- | --- |
| `GET/POST /companies/{id}/invoice-templates` | list / create (`from_invoice_id` = "gjenta denne") |
| `PUT …/invoice-templates/{tid}` | edit incl. lines and aktiv |
| `POST …/invoice-templates/{tid}/generate` | generate every due period now |
| `GET …/invoice-templates/{tid}/runs` | the insert-only generation log |

## Tilbud → ordre → faktura (#31)

- The commercial chain BEFORE the invoice lives **outside the ledger**:
  nothing posts, tilbud are freely editable until akseptert/avslått, an
  ordre is a frozen confirmation. Both reuse the invoice line model, so
  conversion is lossless.
- **Own gap-free number series per kind** (same counter pattern as
  invoices) — a rejected tilbud is history, not a hole.
- **One-way statuses**: tilbud utkast → sendt → akseptert | avslått
  (accepting straight from utkast is allowed); ordre bekreftet →
  fakturert. At most one ordre per tilbud (unique index); converting an
  ordre runs the NORMAL atomic invoice path (number, KID, posting,
  stored PDF) and links the chain tilbud → ordre → invoice — the ordre
  status flip and the invoice commit together. One ordre → one faktura.
- **PDF on demand**: TILBUD/ORDREBEKREFTELSE rendered from current
  state with the same layout (no KID, no betalingsinformasjon — not
  payable); the stored, hash-verified document arrives with the
  invoice.

| Endpoint | Purpose |
| --- | --- |
| `GET/POST /companies/{id}/quotes`, `PUT …/{qid}` | tilbud; edit while utkast/sendt |
| `POST …/quotes/{qid}/status` | sendt \| akseptert \| avslatt |
| `POST …/quotes/{qid}/order` | akseptert tilbud → ordre (lines copied) |
| `GET/POST /companies/{id}/orders` | ordrer (direct creation allowed) |
| `POST …/orders/{oid}/invoice` | ordre → faktura (ordinary path) |
| `GET …/quotes/{qid}/pdf`, `…/orders/{oid}/pdf` | on-demand documents |

## Not yet (deliberate)

- **EHF dispatch** arrives with the Peppol access point (issue #14).
- Proration and seat-based metering — templates are fixed lines first.
- Delleveranser/delfakturering — one ordre → one faktura in v1.

Purring, forsinkelsesrente og inkassovarsel: shipped — docs/purring.md.

## Web API (mutations require bokforing)

| Endpoint | Purpose |
| --- | --- |
| `POST /companies/{id}/invoices` | issue (party_no, dates, lines; defaults: journal GL, receivable 1500, VAT 2700, account 3000, quantity 1). Optional `timer_entry_ids`: selected unbilled hours appended as hour lines per (prosjekt, sats) and marked fakturert in the SAME transaction — one invoice carries products and hours (docs/timer.md; requires `TIMER_FAKTURER`, NOK only) |
| `GET /companies/{id}/invoices?open=true` | list with reskontro remaining per invoice |
| `POST /companies/{id}/invoices/{iid}/credit-note` | full kreditnota |
| `GET /companies/{id}/invoices/{iid}/pdf` | the stored salgsdokument (hash-checked) |
| `GET/PUT /companies/{id}/settings` | firmaopplysninger for the PDF (PUT admin-only) |
| `PUT /companies/{id}/parties/{pid}/contact` | party address/e-mail |

## Where it is tested

- `regnmed-core/src/invoice.rs` — line/VAT computation, rounding, KID
  validity, voucher balance with party, credit-note sign flip.
- `regnmed-api/tests/grupper/invoice.rs` (real Postgres, also CI) — the whole
  loop over HTTP: issue (12 500 kr, valid KID), failed attempt burns no
  number, chain verifies, OCR payment resolves the invoice by KID,
  kreditnota auto-settles, double-credit rejected.
- `regnmed-core/src/pdf.rs` + `fakturapdf.rs` — valid xref structure,
  WinAnsi encoding incl. CP1252 typografi, escaping, width-based right
  alignment, determinism, lovpålagt innhold, kreditnota variant,
  pagination.
- `regnmed-api/tests/grupper/faktura_pdf.rs` (real Postgres, also CI) —
  settings over the API, the PDF exists as a verified attachment the
  moment the invoice does, served with the kontaktinfo, kreditnota
  document, purring `?format=pdf`, settings PUT rejected for
  non-admins.
- `regnmed-api/tests/grupper/salgsdokument.rs` (real Postgres, also CI) — the
  whole chain over HTTP: tilbud edited while utkast, PDF without
  KID/betalingsinfo, one-way trapp, ordre copied losslessly (one per
  tilbud), fakturering through the ordinary path (chain + stored PDF
  verify, links carried), avslått path keeps the number series
  gap-free, direct ordre.
- `regnmed-api/tests/grupper/invoice_template.rs` (real Postgres, also CI) —
  template over the API, catch-up generation with periodetekst through
  the gap-free path (chain + attachments verify), idempotence, run log
  append-only and marked til utsendelse, deactivation respected,
  "gjenta denne" copies customer + lines.
- `regnmed-api/tests/grupper/utsendelse.rs` (real Postgres + a spawned
  `nats-server`, skips without either) — the send endpoint puts a real
  JetStream message on the rail in regnid's wire format (attachment
  base64-decodes back to the stored PDF, reply-to = company),
  the log records it, and an unconfigured rail answers clearly.
  regnid's own suite pins the wire format's backward compatibility.


## Kontantsalg (#89, bokføringsforskriften §5-3)

En **kontantfaktura** dokumenterer en ytelse som er betalt ved levering
— kort, Vipps eller kontanter. Dokumentet er et annet dokument, ikke en
faktura med et flagg: `Dokumenttype::Kontantfaktura` gir tittelen
KONTANTFAKTURA, «Betalt: <betalingsmiddel>» blant dokumentfaktaene, og
**ingen KID, ingen forfallsdato og ingen betalingsinformasjon**. Å be
noen betale det de allerede har betalt er ikke en skjønnhetsfeil, det er
et krav om betaling nummer to. Testen lister de forbudte strengene og
sjekker samtidig at en vanlig faktura beholder dem alle.

**Fordringen oppstår og gjøres opp i SAMME transaksjon.** Det ville vært
enklere å postere salget rett mot bank og hoppe over 1500 — og det er
nettopp det som ikke må skje: reskontro-doktrinen sier at en kundes
posteringer bærer en part, og en sidedør forbi den ville gjort
`reskontro_kontroll` (revisors avstemming) stille ufullstendig. Kravet
finnes, bærer parten på BEGGE sider, og den åpne posten lukkes i det
øyeblikket den oppstår.

`oppgjorskonto` oppgis av kalleren — 1900 kontanter, 1920 bank eller
kortinnløserens oppgjørskonto. Vi gjetter aldri hvordan noen ble betalt.

`POST /companies/{id}/invoices` med `kontant_betalingsmiddel` +
`oppgjorskonto` tar denne veien; uten dem er alt som før.
