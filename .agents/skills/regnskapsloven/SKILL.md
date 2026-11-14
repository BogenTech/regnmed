---
name: regnskapsloven
description: "Norwegian accounting law applied to code — bokføringsloven/bokføringsforskriften, regnskapsloven, god bokføringsskikk (NBS/GBS), god regnskapsskikk (NRS), merverdiavgift, SAF-T Financial, and mandatory e-faktura (EHF/Peppol). REQUIRED reading before writing, reviewing or designing any code in RegnMed, NG Pos or another Norwegian ledger/invoicing/POS system that touches vouchers (bilag), postings, the general ledger, customer/supplier ledgers (reskontro), invoices and credit notes, VAT codes or the mva-melding, period closing, corrections, deletion or retention of accounting data, backups or storage location, audit trails, SAF-T export, year-end/annual accounts, or posting Z-reports from a POS. Also use when asked whether something is legal or 'god regnskapsskikk' in Norwegian accounting, even if the user doesn't name the law. Triggers: regnskapsloven, bokføringsloven, bokføringsforskriften, god bokføringsskikk, god regnskapsskikk, NRS, NBS, GBS, bilag, bilagsnummer, hovedbok, reskontro, kontrollspor, ajourhold, periodelukking, retting, storno, kreditnota, salgsdokument, faktura, e-faktura, EHF, Peppol, ELMA, mva, merverdiavgift, mva-melding, mva-kode, SAF-T Regnskap, standard kontoplan, oppbevaring, årsregnskap, årsoppgjør, Regnskapsregisteret, Z-rapport bokføring, kassaoppgjør."
---

# Norwegian accounting law for RegnMed, NG Pos and friends

Software that keeps books for a Norwegian business is the instrument through
which that business meets bokføringsloven. The law binds the business, not
the vendor — but a business cannot comply using a system that makes
compliance impossible, and from **1 January 2030** bookkeeping *must* happen
in an `elektronisk regnskapssystem` (bokføringsloven § 7 fourth paragraph,
new by LOV-2026-06-19-39). So a design defect here is a legal defect for
every customer who runs the code.

Treat this skill as a boundary, not a style guide. When the law and a
convenient design disagree, the law wins and the conflict is reported.

**Sibling skills.** For cash-register specifics (electronic journal, receipt
headings, X/Z-report contents, produkterklæring) read **kassaloven**. For how
NG Pos represents money, read **money-arithmetic**. This skill covers what
happens to those numbers once they are accounting data.

## The one test to apply first

> **The books may not be able to lie about what happened, or forget it
> before the law allows.**

Almost every rule below is a consequence: postings are documented, traceable
both ways, complete, closed on time, corrected only by visible new entries,
and retained in readable form. When a design question is ambiguous, ask
"could this let the books show something other than what happened, or lose
it early?" If yes, it is probably illegal.

## Map of the rulebook

| Instrument | Governs | Who it binds | Typical code impact |
|---|---|---|---|
| Bokføringsloven (LOV-2004-11-19-73) + bokføringsforskriften (FOR-2004-12-01-1558) | Day-to-day bookkeeping: vouchers, specifications, control trail, corrections, closing, retention | All `bokføringspliktige` (AS, ENK above thresholds, etc.) | Data model, immutability, numbering, audit trail, export, storage |
| God bokføringsskikk — NBS standards and GBS statements (Regnskapsstiftelsen) | Authoritative interpretation of the above | Same (via § 4 nr. 10) | NBS 1 sikring, NBS 2 kontrollspor, NBS 3 3.5-year access, NBS 5 balance documentation, NBS 6 spreadsheets, NBS 8 side specs; GBS 1 kreditnota |
| Regnskapsloven (LOV-1998-07-17-56) | Annual accounts: principles, valuation, layout, notes, filing | `regnskapspliktige` (AS, ASA, larger partnerships …) | Year-end, reports, closing entries, size categories |
| God regnskapsskikk — NRS (e.g. NRS 8 for small companies) | Valuation/presentation practice | Same (§ 4-6) | Report layout, accrual rules |
| Merverdiavgiftsloven + skatteforvaltningsloven | VAT: rates, codes, periods, mva-melding | VAT-registered businesses | VAT codes, VAT specification, mva-melding API |
| SAF-T Financial (Skatteetaten) | Standardized export of bookkeeping data on request | Bokføringspliktige with electronic books | Export format, standard accounts and tax codes |
| E-invoicing rules (bokføringsloven § 10 second paragraph, forskrift § 5-2-9) | Structured invoice format B2B | Bokføringspliktige selling to bokføringspliktige | EHF/Peppol issuing and receiving, original-format storage |

## Bokføringsloven § 4 — the ten principles as design rules

`Bokføring, spesifikasjon, dokumentasjon og oppbevaring` must follow these
principles. Each one has a concrete engineering consequence:

1. **Regnskapssystem** — the system must be able to produce the mandatory
   reporting (`pliktig regnskapsrapportering`, forskrift § 2-1: annual accounts, mva-melding,
   tax return etc.) and its specifications, and be arranged so the duty to
   provide information can be met. → Every report the business must file is
   reproducible from the ledger, not from a cache or a spreadsheet.
2. **Fullstendighet** — all transactions are recorded. → No silent drops,
   no "skip" flags, no filters that remove postings from totals.
3. **Realitet** — postings reflect actual events or accounting judgements
   of *this* entity. → Strict tenant/legal-entity separation; no shared
   ledgers across organisasjonsnumre.
4. **Nøyaktighet** — correct and exact. → Exact decimal arithmetic (Postgres
   `NUMERIC`, never floats); every voucher balances (sum debit = sum credit)
   in bookkeeping currency.
5. **Ajourhold** — booked as often as the nature and size of the business
   requires. → See deadlines below; the system must make timely posting easy
   and show what is overdue.
6. **Dokumentasjon** — every posting is documented in a way that shows its
   justification. → Every journal entry references a `bilag`; the bilag is
   stored, not just its number.
7. **Sporbarhet** — two-way control trail from documentation via
   specifications to the mandatory reporting. → Drill-down and drill-up must
   both work: report line → accounts → postings → bilag, and back.
8. **Oppbevaring** — material is retained for the required time in readable
   form. → See retention table.
9. **Sikring** — protected against unauthorised alteration, deletion or
   loss. → Access control, closed periods, backups, tamper evidence.
10. **God bokføringsskikk** — everything above is done according to good
    bookkeeping practice. → NBS/GBS are not optional reading; they are how
    regulators and auditors interpret the law.

## Posting, numbering and the control trail (§§ 5, 6, 7, 10)

- **Bilag numbering.** Vouchers are numbered sequentially (§ 6). Several
  series are fine (e.g. one per voucher type), but NBS 2 says there must be
  no holes in the total series when periods are viewed together. Do not
  burn numbers on drafts: assign the voucher number at posting, not at
  draft creation, or make the draft series separate and visibly not bilag.
  A Postgres `SEQUENCE` alone gaps on rollback — use a counter row updated in
  the same transaction as the posting, or document and explain gaps.
- **Specifications (§ 5, forskrift § 3-1).** At least within the reporting
  deadlines, and not less often than every four months, the system must be
  able to produce: `bokføringsspesifikasjon` (journal in voucher order),
  `kontospesifikasjon` (general ledger per account), `kundespesifikasjon`,
  `leverandørspesifikasjon`, `spesifikasjon av uttak`, and the further
  specifications in § 3-1 (VAT specification among them). Read § 3-1
  verbatim before implementing the list — this file compresses it.
- **System-generated postings** (allocations, automatic VAT, currency
  revaluation, bank import) must be explainable: § 6 requires documentation
  of the accounting system describing how system-generated postings can be
  verified, including relevant codes and fixed data. Keep rule versions and
  the inputs that produced each automatic entry.
- **Bookkeeping currency** is NOK unless the forskrift allows otherwise
  (§ 8, forskrift § 4-2). Store original currency, amount, rate and rate
  source per posting line.
- **Ajourhold deadlines (§ 7 second paragraph, forskrift § 4-1).** Postings
  must be up to date within the deadlines for the mandatory reporting (e.g.
  the VAT term) and not less often than every four months. Businesses with
  fewer than 600 vouchers a year may instead keep up within the reporting
  deadlines.

## Corrections and period closing (§ 9, forskrift § 7-6)

This is where accounting software most often breaks the law.

- **After the § 7 deadlines, booked information must not be changed or
  deleted (§ 9).** Corrections are made with a new, documented correcting
  entry (reversal/`storno` plus re-posting), referencing the original.
- **Periods must be closed** within the § 7 deadlines in a way that gives
  *betryggende sikring* against change or deletion (forskrift § 7-6), and a
  description of how closing works must be retained for five years. A
  closed-period flag that an admin UI can flip back is not closing.
- **Before closing**, edits are still allowed but must stay traceable:
  record who changed what, when and from what. When information is deleted,
  that must be visible in the documentation or specification (§ 9).
- **No hard deletes of vouchers or postings** during the retention period —
  not via UI, not via API, not via "GDPR delete user". Personal data in
  regnskapsmateriale is retained on the legal-obligation basis until the
  retention period ends; design deletion jobs around that.
- Migrations and data fixes that rewrite posted rows are themselves
  corrections and must go through the same correcting-entry path, or be
  explicitly approved with a written record.

## Documentation of transactions (§ 10, forskrift kap. 5)

- **Sales documents** (`salgsdokument`, forskrift § 5-1-1) have mandatory
  contents — number and documentation date, seller name and
  organisasjonsnummer (+ `MVA` if VAT-registered), buyer, delivery date and
  place, nature and extent of the supply, consideration and due date, VAT
  amount. Read § 5-1-1 and the issuing rules in delkapittel 5-2 verbatim
  before building an invoice template.
- **From 1 January 2028:** the buyer's organisasjonsnummer is always required
  when selling to a bokføringspliktig buyer (§ 5-1-1/5-1-2), and parties may
  no longer agree that the sales document is not sent (§ 5-2-1 fourth
  paragraph).
- **Credit notes** are documents in their own right with the same content
  rules, referencing the original invoice; never edit or "cancel" an issued
  invoice. See GBS 1 *Kreditnota* (updated January 2026).
- **Cash sales** are documented via the cash register: the Z-report is the
  voucher for the day's cash sales per kassapunkt (delkapittel 5-3/5-4; see
  POS section).
- **Self-produced documentation** (accruals, depreciation, internal
  transfers) still needs a bilag stating what, why, and who.

## E-invoicing (bokføringsloven § 10 second paragraph; forskrift § 5-2-9)

Staged by LOV-2026-06-19-39 and FOR-2026-09-29-1933:

| Date | Obligation |
|---|---|
| 1 Jan 2027 | Must **issue** e-faktura when selling to a bokføringspliktig buyer that can receive it (registered in ELMA). Approved formats: EHF Fakturering / Peppol BIS Billing and the self-billing variants, version 3.0 or later. Received e-invoices kept in **original format** (§ 13 second paragraph). |
| 1 Jan 2027 – 31 Dec 2029 | Transitional: other structured formats (e.g. EDIFACT) by agreement. **PDF by email is not an e-invoice.** |
| 1 Jan 2028 | Buyer org.nr. mandatory; sales document must always be sent to bokføringspliktig buyers. |
| 1 Jan 2030 | Must **receive** e-invoices; bookkeeping must happen in an electronic accounting system. |

Code consequences: an ELMA lookup in the invoice flow; storing the XML as the
bilag (a rendered PDF is a convenience view, not the original); validation
against EHF/Peppol BIS 3.0 rules before sending. A low-turnover exemption
(forskrift § 1-2, ≤ NOK 50,000 with further conditions) exists — model it as
a flag, don't hardcode it away.

## Retention and access (§§ 13, 13a, 13b; forskrift kap. 7)

| Material | Period (after end of financial year) |
|---|---|
| Annual accounts, mandatory reporting, specifications needed for them, vouchers (bilag), deleted-information documentation, control-trail and balance documentation, numbered auditor letters | **5 years** |
| Supplementary documentation (agreements, significant correspondence, packing slips, price lists etc., § 13 first paragraph nr. 5–8) | **3.5 years** |
| Booked information originally electronic: must be **electronically accessible** and reproducible in standard form (SAF-T, forskrift § 7-8) | **3.5 years** (§ 13b) |
| Special cases (construction project accounts, certain bank specs) | 10 years — see forskrift kap. 8 |

- The small-business exemption from 3.5-year electronic availability (turnover
  under NOK 5 million, forskrift § 7-7) **is removed from 1 January 2027**.
- Storage: electronic material may be stored in Norway, any EEA state, the
  UK or Switzerland (forskrift § 7-5, as amended 27 January 2025) — check the
  cloud region of the database *and* the backups (§ 7-2). Storage abroad is
  not free-form: the bokføringspliktige must give **written notice to
  skattekontoret** (no dispensation needed inside EEA/UK/CH), and the tax
  office can order storage back in Norway for up to 3 years after serious
  breaches. For a SaaS vendor this means: document the region in the
  customer-facing system description and give customers what they need to
  send that notice. Anywhere else (e.g. US regions) requires a dispensation —
  treat it as not allowed.
- Storage must preserve readability for the whole period (forskrift § 7-1),
  be secured (NBS 1), and the business must be able to hand over data on
  demand. Format migrations must not lose bilag or history.
- A business that winds down must keep electronic access for a period after
  cessation (see NBS 3) — design export-and-archive for tenants who leave.

## VAT (merverdiavgift)

- Rates for 2026 and proposed for 2027: **25 %** general, **15 %** food,
  **12 %** passenger transport, hotel and certain culture/sport, **11.11 %**
  wild marine resources, plus zero-rated and exempt supplies. Rates change by
  Stortinget decision — store them as dated configuration, never as
  constants.
- Each posting with VAT carries a VAT code that maps to Skatteetaten's
  **standard tax codes** (SAF-T / mva-melding). Internal codes are fine;
  the mapping must be explicit and versioned.
- Reverse charge (`omvendt avgiftsplikt`) on purchases of services from
  abroad and certain domestic cases produces both output and input VAT —
  model it as a code pair, not a manual double entry.
- The **mva-melding** is submitted from the system via Skatteetaten's API
  (Altinn 3). Altinn II roles are removed from 1 January 2027; integrations
  must use the new access packages (`tilgangspakker`).
- Errors in a submitted term are corrected by a new mva-melding for that
  term, never by quietly re-posting into a closed term.
- The VAT specification must reconcile to the mva-melding; differences must
  be explained (forskrift § 3-1).

## SAF-T Financial

- The business must be able to produce SAF-T Financial on request
  (forskrift § 7-8), for any period within the 3.5-year window.
- **Version 1.40 is the only valid format from 1 January 2027.** 1.30 (and
  1.20) only for financial year 2026 and earlier — so the exporter must be
  able to produce more than one version.
- Accounts map to Skatteetaten's standard accounts, VAT codes to standard tax
  codes; customers/suppliers come from the reskontro master data.
- Validate exports against the official XSD before calling them done.

## Regnskapsloven — annual accounts

- **Size categories** now sit in regnskapsloven **§ 1-5** (micro, small,
  medium, large — changed with effect from 1 November 2024). Do not hardcode
  the old § 1-6 thresholds; read the current amounts.
- **§ 4-1 basic principles**: transaction, earnings (opptjening), matching
  (sammenstilling), prudence (forsiktighet), hedging (sikring), and
  ongoing-concern/consistency rules in §§ 4-2–4-5; **§ 4-6** requires *god
  regnskapsskikk*. Small companies may deviate from some (§ 4-1 second
  paragraph) — the system should support both, chosen per entity.
- **NRS 8** is god regnskapsskikk for small companies (last updated December
  2025: extraordinary items removed from the income statement layouts,
  references to the new size categories).
- Layouts follow the oppstillingsplaner in kapittel 6 (by nature/by
  function). Generate them from account mapping, not hand-built queries.
- Annual accounts are adopted and filed with Regnskapsregisteret
  (Brønnøysund) by the deadlines in §§ 3-1 and 8-2 — check the current
  wording before building deadline reminders.
- Year-end closing entries are vouchers like any other, numbered and
  documented. The opening balance of the next year must equal the closing
  balance, and that must be provable.
- Pending: Prop. 106 L (2025–2026) amends regnskapsloven on sustainability
  reporting (Omnibus I) — largely irrelevant to ledger code, but watch it.

## POS → ledger (NG Pos and similar)

Only relevant when the task actually involves a cash register or POS data.
Do not bring Z-reports into reviews of general ledger code.

- The **Z-report** is the voucher for cash sales at a cash point for that day.
  Post one bilag per Z-report, referencing its Z number, with sales split per
  VAT rate and settlement split per payment means (cash, card, vipps, gift
  card, credit sale …). The X-report is an interim view and is not posted.
- Post the **cash count** (`kasseoppgjør`/telling) separately and book the
  difference (`kassedifferanse`) explicitly. Never adjust sales to make the
  count match.
- Returns and corrections arrive from the POS as their own registrations
  (kassaloven forbids mutating them); the ledger mirrors that — no netting
  away of a return by editing the original sale.
- Gift cards and deposits are liabilities, not revenue, when sold.
- If a Z-report is posted wrongly, correct it in the ledger with a reversal
  and repost — the Z-report itself stays untouched.

## Review checklist

Before completing work that touches accounting data, verify:

- [ ] Every posting references a stored bilag; every bilag balances.
- [ ] No code path updates or deletes posted entries after the period is
      closed; corrections are reversal + repost, linked to the original.
- [ ] Pre-closing edits record who, when and before/after values.
- [ ] Period closing is enforced in the database or domain layer, not only in the UI,
      and reopening (if any) is itself logged and exceptional.
- [ ] Bilag numbers are sequential with no unexplained gaps; drafts don't
      consume real numbers.
- [ ] Two-way drill: report → account → posting → bilag and back.
- [ ] Amounts are exact decimals; VAT rounding is per documented rule.
- [ ] Tenant/legal-entity isolation holds for every query and export.
- [ ] No retention-period data can be hard-deleted (including via GDPR
      flows, tenant offboarding, or cleanup jobs).
- [ ] Storage and backups are in Norway/EEA/UK/Switzerland, and customers
      know the location so they can notify skattekontoret (forskrift § 7-5).
- [ ] Received e-invoices stored in original XML; outgoing B2B invoices can
      be EHF/Peppol BIS 3.0; buyer org.nr. captured.
- [ ] VAT codes map to standard tax codes; rates are dated config.
- [ ] SAF-T export validates against the version required for the period.
- [ ] Automatic postings are reproducible from stored inputs and rule
      versions, and that behaviour is documented.

## Honesty rules

- This file is a working summary as of November 2026. Enumerated lists
  (forskrift § 3-1, § 5-1-1, regnskapsloven § 4-1, oppstillingsplaner,
  threshold amounts) are compressed here and must be read verbatim from
  Lovdata before being implemented. `references/sources.md` lists where.
- State the paragraph you rely on, or say you have not verified it. Never
  declare a system "compliant": a requirement is either demonstrably met with
  a citation or it is open.
- When you find code that violates a rule here, report it plainly and early,
  with the paragraph — even if the user asked about something else.
- Accounting judgement (valuation, accruals, tax treatment of a specific
  transaction) belongs to the customer's regnskapsfører/revisor. The system
  must support their choices; it must not silently make them.
