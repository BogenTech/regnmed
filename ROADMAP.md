# regnmed roadmap — til norsk regnskaps-MVP og videre

Where we are headed: a full accounting platform for the Norwegian market that
competes with the established actors (Fiken, Tripletex, Conta, Visma,
PowerOffice, Unimicro) on three things they can't easily copy:

1. **The trust story** — a tamper-evident, independently verifiable ledger
   ("don't trust us — verify"), which is also the pitch to revisorer.
2. **The regnskapsfører channel** — accountants bring portfolios; win one
   firm, win its clients.
3. **Cost** — the lowest resource footprint in the market. Frugality is a
   strategy, not an accident: every service must run well in tiny containers
   (the entire platform runs in a 2 GB local VM, and that constraint is kept
   on purpose). Rust + static binaries + SQL-first design keep the marginal
   cost per client near zero, which becomes pricing power.

Interop is the fourth pillar: **easy in, easy out**. SAF-T both directions,
open APIs, no lock-in. Migration *into* regnmed must be painless from every
major Norwegian system — that is the growth lever.

Tracking: every item below has a GitHub issue; milestones M1–M6 mirror the
phases here. This document is the narrative; issues are the work.

---

## M1 — Lovpålagt kjerne (the compliance MVP)

What bokføringsloven and bokføringsforskriften demand of any system that
keeps books in Norway. This *is* the MVP — nothing ships to a paying customer
before M1 is complete. The ledger core (append-only, hash-chained, gap-free
voucher numbers) is done; these build on it.

- **SAF-T Financial v1.3 export** (next up). Mandatory on request from
  Skatteetaten since 2020. Validates our whole data model: kontoplan mapped
  to standard accounts (NS 4102), customers/suppliers, mva-koder, all
  vouchers. Validate output against the official XSD in CI.
- **Standard mva-koder end-to-end.** SAF-T VAT codes on every entry,
  automatic beregning (utgående/inngående, forholdsmessig fradrag later),
  mva-spesifikasjon per termin.
- **Reskontro** — kunde- og leverandørspesifikasjon (subledgers), required by
  bokføringsforskriften §3-1. Open-item matching (åpne poster).
- **Lovpålagte spesifikasjoner og rapporter:** bokføringsspesifikasjon,
  kontospesifikasjon (hovedbok), saldobalanse, resultatregnskap og balanse
  grouped per NS 4102.
- **Periodelåsing.** Posting periods with ajourholdsfrister; closed periods
  reject postings (corrections go in open periods as reversing vouchers,
  which the ledger already enforces).
- **Bilagsvedlegg med oppbevaringsplikt.** Attachment storage (PDF/images)
  hash-bound to the voucher chain, immutable, retained 5 år
  (bokføringsloven §13). The attachment hash goes into the voucher's
  canonical content so tampering with documentation is provable too.

## M2 — Offentlige integrasjoner (Altinn / Skatteetaten / registre)

The government rail. Everything here rides on **Maskinporten** (machine-to-
machine auth) and **Altinn Autorisasjon** (who may act for which orgnr —
which maps 1:1 onto our engagement model).

- **Maskinporten foundation.** Client registration, JWT grant, scope
  handling, and delegation so a regnskapsfører submits on behalf of clients.
  One shared crate — every government API below reuses it.
- **Mva-melding.** Compile from the ledger (per termin, from mva-koder),
  validate against Skatteetatens validation API, submit via the
  mva-melding innsending API (Altinn 3), store the receipt/reference as
  ledger evidence. This is the first "wow" for accountants.
- **BRREG / Enhetsregisteret onboarding.** Orgnr lookup, company facts,
  roles — already planned as the company-creation flow.
- **Finanstilsynets virksomhetsregister.** Verify autorisasjon
  (regnskapsfører/revisor) before anyone offers services in the marketplace.
- Later in this track (own issues, not MVP): **skattemelding med
  næringsspesifikasjon** (annual tax return as sluttbrukersystem),
  **årsregnskap til Regnskapsregisteret**. **A-melding (payroll) stays
  deliberately deferred for years.**
- **Lovdata:** no public free API; we link to regulations (kontohjelp-style
  references from mva-koder and reports) rather than integrate. Revisit only
  if a licensed need appears.

## M3 — Faktura og bank

Money in, money out — the daily-driver features.

- **Utgående faktura** with KID (MOD10/MOD11), credit notes, purring.
  Posting straight to the ledger + reskontro.
- **EHF / Peppol** send and receive through an access point (evaluate
  provider vs. certifying our own AP later; provider first — frugal).
  Mandatory for B2G, expected in B2B.
- **Bankavstemming.** Import camt.053 (ISO 20022) and CSV; matching engine
  against reskontro and KID/OCR references.
- **OCR-giro innbetalinger** (Mastercard Payment Services/Nets file feed) for
  automatic KID matching.
- Later: pain.001 remittering (payments out), PSD2/open banking live feeds.

## M4 — Migrering fra andre systemer (the growth lever)

"Switching is painless" is the sales pitch to every accountant with an
existing portfolio.

- **SAF-T import** — the universal path. Every Norwegian system is required
  to export SAF-T, so one great importer covers Visma, Tripletex, Fiken,
  Conta, PowerOffice, Unimicro at once: kontoplan, åpningsbalanse,
  customers/suppliers, historical vouchers (imported as a sealed
  opening-history journal so our chain starts clean).
- **Kontoplan-mapping + åpningsbalanse wizard.** Interactive mapping of
  their accounts onto NS 4102, validated debit=credit before sealing.
- **Native API importers** for what SAF-T lacks: open reskontro items,
  attachments/bilagsbilder, recurring invoices, contacts. Priority order by
  market share of the accountant segment: **Tripletex, Fiken, Visma
  eAccounting, PowerOffice, Conta** (all have public APIs).

## M5 — Portal og marketplace

- **Portal UI** — Tailwind v4 + daisyUI 5, shared theme contract with regnid
  (`../regnid/ui/themes.css`); bilagsregistrering, reports, reskontro views.
- **Bilagsinnboks** — upload/e-mail-in of documentation, accountant workflow
  (klient laster opp, regnskapsfører posterer).
- **Engagement management UI** — oppdrag lifecycle (scope, validity,
  takeover/opphør), firm membership admin.
- **Accountant directory** — verified autorisasjon badges, businesses find
  and engage accountants; the marketplace itself.
- **Revisorrolle** — read-only engagement + one-click chain verification
  report (the trust story productized).

## M6 — Tillit og skala

- ✅ **External anchoring** of chain heads so even DBA-level tampering is
  provable: nightly Merkle snapshots, public `/anchors` root feed,
  RFC 3161 witness tokens, per-company inclusion proofs
  (docs/anchoring.md). Follow-up: attachment-set binding (leaf v2).
- **regnid production-ready:** OpenID conformance suite, then **ID-porten
  federation** so businesses log in with what they already have.
- **Production deploy:** k8s overlays on top of `deploy/local`, TLS,
  backups/PITR, observability — all within the frugality budget (measure
  per-service RSS in CI; a service that grows fat fails the build).

---

## Rekkefølge / dependencies

```
M1 SAF-T export ─┬─▶ M2 mva-melding (needs mva-koder + terminer)
                 ├─▶ M4 SAF-T import (shares the SAF-T model crate)
M1 reskontro ────┼─▶ M3 faktura/bank (needs open items)
M2 Maskinporten ─┴─▶ every later government API
M5 portal ───────▶ marketplace (needs BRREG + Finanstilsynet from M2)
```

M1 and the Maskinporten foundation can run in parallel. Within M1, SAF-T
export comes first because it forces every data-model decision early.
