# Betalingsoppfølging (purring)

Payment follow-up built on reskontro's åpne poster: overdue invoices
are always **computed** from `due_date` + reskontro remaining — never
stored state — and only the purreskritt themselves are rows.
Everything regelverksstyrt comes from the satsregister
(docs/regelverk.md); the rules are applied purely in
`regnmed-core::purring`.

regnmed stops at inkassovarselet: inkasso is bevillingspliktig
(inkassoloven) and is handed off, stated in the document itself.
Sending a purring is always an explicit human action — the system
suggests and computes, a person clicks.

## Guarantees

- **Gebyr og rente er bilag, aldri sidestilte gebyrer.** When a skritt
  demands purregebyr and/or forsinkelsesrente, one voucher posts in the
  same transaction as the skritt is recorded (debit the reskontro
  receivable *with the customer* — the krav becomes an åpen post on the
  same reskontro; credit income accounts, default 3950/8050, no VAT —
  utenfor mva-loven). A failed posting leaves no skritt (the
  bilagsinnboks pattern). The original invoice's remaining is untouched
  until payment.
- **Forsinkelsesrente per forsinkelsesrenteloven §2**: runs from the day
  after forfall, actual days / 365, **segmented across the halvårlige
  satser** in the satsregister — each period rounded half away from
  zero, so the spesifikasjon in the document sums exactly to the total.
  A day without a covering sats fails loudly; rates are never guessed.
- **Purregebyr per inkassoforskriften §1-2**: earliest 14 days after
  forfall, capped at `purregebyr_maks` (or `standardkompensasjon` for
  næringsdrivende skyldnere, forsinkelsesrenteloven §3a) valid on the
  sending date, at most two gebyrbelagte skritt per krav. A
  betalingspåminnelse is gebyrfri by definition.
- **Inkassovarsel per inkassoloven §9**: minimum 14 days
  betalingsfrist, enforced; the document carries the lovtekst and the
  hand-off statement.
- **Purretrappen er enveis**: betalingspåminnelse → purring →
  inkassovarsel; a milder skritt after a stronger one is rejected.
- **The history is evidence**: `invoice_reminder` (migration 0017) is
  insert-only (append-only triggers + INSERT/SELECT-only grants), and
  the **rendered document is stored at registration** — the krav is
  reproducible byte for byte forever, however satser change later.
  Rendering is deterministic (`render_dokument`), same doctrine as the
  verifikasjonsrapport.
- **KID follows the invoice**: the krav carries the original invoice's
  KID, so an OCR payment still resolves the right invoice.

## Aldersfordeling

`GET …/invoices/overdue` buckets forfalte fakturaer 1–14 / 15–30 / 30+
days (kreditnotaer never count), with each invoice's last skritt. The
portal shows the buckets in the Faktura section and a Forfalt stat on
Oversikt.

## Web API (mutations require bokforing)

| Endpoint | Purpose |
| --- | --- |
| `GET /companies/{id}/invoices/overdue` | forfalte with buckets + siste skritt |
| `GET /companies/{id}/invoices/{iid}/reminders` | insert-only history |
| `POST /companies/{id}/invoices/{iid}/reminders` | register a skritt (steg, frist_date, gebyr_ore?, med_rente?, naeringsdrivende?; accounts overridable). `?preview=true` computes gebyrtak, rente and document without writing |
| `GET /companies/{id}/invoices/{iid}/reminders/{rid}?format=tekst` | the stored document |

## Not yet (deliberate)

- No automatic sending — delivery (PDF/e-post, #32) will still require
  a human click per the issue.
- Rentenota on payment date (rente accrued after the last skritt) —
  today rente is crystallized per skritt when demanded.

## Where it is tested

- `regnmed-core/src/purring.rs` — pinned rente across a satsskifte
  (10 000 kr, 15+15 days → 51,37 + 50,34 kr), no rente t.o.m. forfall,
  loud failure without sats coverage, the full stegregel matrix (14-day
  rules, gebyrtak, maks to gebyr, enveis trapp), voucher balance,
  deterministic rendering incl. the inkassovarsel lovtekst.
- `regnmed-api/tests/purring.rs` (real Postgres, also CI) — the loop
  over HTTP: aldersfordeling buckets, gebyrfri påminnelse posts
  nothing, purring with gebyr + rente posts one voucher and the chain
  verifies, gebyr over maks / short inkassovarsel-frist / steg tilbake
  rejected, history immutable (UPDATE rejected by trigger).
