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
- ✅ **Lovpålagte spesifikasjoner og rapporter:** bokføringsspesifikasjon,
  kontospesifikasjon (hovedbok), saldobalanse, resultatregnskap og balanse
  grouped per NS 4102 — pure SUM queries, `/reports/*` endpoints, portal
  Rapporter section (docs/rapporter.md).
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
- ✅ **Bankavstemming.** camt.053 (ISO 20022) and header-detected bank CSV
  through one import endpoint and one matching engine; idempotent
  re-import in both tiers (docs/bank.md).
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
- ✅ **Kontoplan-mapping + åpningsbalanse wizard.** Analyze-first import
  suggests NS 4102 mappings (heuristics; the admin decides), merges
  same-target accounts, enforces 4-digit results; manual åpningsbalanse
  endpoint for systems without SAF-T, zero-sum enforced
  (docs/migration.md).
- **Native API importers** for what SAF-T lacks: open reskontro items,
  attachments/bilagsbilder, recurring invoices, contacts. Priority order by
  market share of the accountant segment: **Tripletex, Fiken, Visma
  eAccounting, PowerOffice, Conta** (all have public APIs).

## M5 — Portal og marketplace

- **Portal UI** — Tailwind v4 + daisyUI 5, shared theme contract with regnid
  (`../regnid/ui/themes.css`); bilagsregistrering, reports, reskontro views.
- ✅ **Bilagsinnboks** — klient laster opp (immutable fra ankomst),
  regnskapsfører bokfører: bilag + vedlegg + status i én transaksjon, eller
  avviser med notat (docs/bilagsinnboks.md). E-mail-in/OCR is a later
  enhancement on the same endpoint.
- **Engagement management UI** — oppdrag lifecycle (scope, validity,
  takeover/opphør), firm membership admin.
- **Accountant directory** — verified autorisasjon badges, businesses find
  and engage accountants; the marketplace itself.
- ✅ **Revisorrolle** — read-only engagement + one-click verification
  report: six kontroller (chain, attachments, anchors, reskontro tie-out,
  balance, period locks) + deterministic text download (docs/revisjon.md).

## M6 — Tillit og skala

- ✅ **External anchoring** of chain heads so even DBA-level tampering is
  provable: nightly Merkle snapshots, public `/anchors` root feed,
  RFC 3161 witness tokens, per-company inclusion proofs
  (docs/anchoring.md). Follow-up: attachment-set binding (leaf v2).
- **regnid production-ready:** OpenID conformance suite, then **ID-porten
  federation** so businesses log in with what they already have.
- ✅ **Production deploy scaffolding:** base + overlays (local render
  byte-identical), prod overlay with cert-manager TLS, secrets
  out-of-band, pinned images, TSA-witnessed anchoring, and backups with
  a weekly unattended restore-verification that re-walks the restored
  ledger's hash chains (docs/deploy.md). PITR via CloudNativePG is the
  documented growth path; first real cluster still pending (domain,
  registry, hosting decision).
- ✅ **Frugality budget in CI:** release binary sizes + regnmed-api peak
  RSS under load, gated against hard budgets in every build; the RSS
  budget equals the k8s container limit (scripts/frugality.sh,
  docs/frugality.md — today: 11 MB binary, 11 MB peak RSS).

---

## M7 — Funksjonsbredde: daglig drift

The capabilities an SMB expects of a complete accounting system beyond
the compliance core — mapped as issues 2026-07-23 after a systematic
gap review of the Norwegian SMB market, each written from user need +
Norwegian regulation and regnmed's principles (immutability, API-first,
frugality). Filed under M2/M3/M5 where they extend existing themes,
M7 (#37–#48) for new ground:

- Betalingsoppfølging: ✅ purring m/ gebyr og forsinkelsesrente (#29 —
  aldersfordelte forfalte, gebyr/rente som ordinære bilag fra
  satsregisteret, inkassovarsel m/ 14-dagers frist, insert-only
  historikk m/ lagret dokument; docs/purring.md),
  ✅ repeterende faktura (#30 — editable maler, generering som
  ordinære fakturaer m/ periodetekst via daglig CronJob, insert-only
  kjøringslogg, «merk for utsendelse» — mennesket sender;
  docs/faktura.md), ✅ tilbud → ordre → faktura (#31 — kjeden utenfor
  hovedboken m/ egne nummerserier, enveis statuser, tapsfri
  konvertering og ordinær fakturering; docs/faktura.md),
  ✅ faktura-PDF + e-postutsendelse (#32 — deterministisk hand-rolled
  PDF lagret på bilaget ved utstedelse, e-post med vedlegg over
  regnids mail-rail m/ insert-only utsendelseslogg; docs/faktura.md)
  — M3.
- Penger ut: ✅ betalingsliste + pain.001-remittering (#33 —
  enveis kjøringer m/ separat opprettelse og godkjenning, kreditordata
  kopiert på radene, XSD-validert pain.001 lagret m/ hash, oppgjør
  som bokfører og lukker reskontroen i én transaksjon,
  MOD11-validerte kontonumre; docs/betaling.md),
  ✅ utlegg og kjøregodtgjørelse (#42 — uforanderlige krav m/
  hash-sjekket kvittering som følger bilaget, enveis
  innsendt→godkjent/avvist→utbetalt, km × statens sats fra
  satsregisteret m/ trekkpliktig del som tydelig varsel, mva-splitt
  ved godkjenning, utbetaling mot mellomregning; docs/utlegg.md),
  ✅ attestering/four-eyes (#47 — valgfri datert policy m/
  beløpsgrense og utpekt attestant, insert-only beslutningsspor der
  nyeste beslutning gjelder, bokføring/betalingsgodkjenning/
  utleggsgodkjenning nektet for samme person — håndhevet inne i
  transaksjonene, ikke i portalen; docs/attestering.md).
- Innboks-produktivitet: bilagstolkning som forslag (#34), e-post-inn
  (#35) — M5.
- Innsikt: ✅ nøkkeltall/likviditet (#36 — resultat hittil mot samme
  periode i fjor, månedskolonner som rene CSS-søyler, likviditetsbilde
  m/ beregnet mva-netto, kommende frister etter terminordningen;
  docs/rapporter.md), budsjett m/ fastsatte versjoner
  (#41) — pure queries as always.
- Struktur: ✅ dimensjoner prosjekt/avdeling — hash format v3 (#37 —
  registry insert/rename/open-close only, koder hash-dekket, avsluttet
  avviser posteringer, resultat per dimensjon, SAF-T Analysis;
  docs/dimensjoner.md),
  ✅ timeføring (#38 — heltallsminutter, månedslås som bevisgrense
  håndhevet i databasen, fakturagrunnlag per prosjekt gjennom den
  ordinære fakturaflyten m/ prosjektdimensjon; docs/timer.md),
  ✅ produktregister + enkelt varelager (#39 — verdier kopieres til
  linjene ved utstedelse, insert-only lagerbevegelser m/ automatisk
  salg/retur fra fakturaflyten, gjennomsnittskost som ren funksjon,
  varetelling m/ bilag mot bokført saldo; docs/produkter.md),
  ✅ anleggsregister m/ avskrivninger (#40 — registeret som bevis
  (insert + enveis avhending), lineære månedsbilag via månedlig
  CronJob m/ insert-only logg, skattemessig saldo per gruppe a–j fra
  satsregisteret m/ midlertidige forskjeller, avhending m/
  gevinst/tap; docs/anlegg.md),
  ✅ flervaluta (#44 — hash format v4: valutabeløp og bokført kurs på
  linjene som bevis; daterte kurser fra Norges Banks åpne API m/
  UNIT_MULT-håndtering og kilde per rad; faktura i valuta bokført i
  NOK til dagskurs; realisert agio i SAMME transaksjon som
  valutamatchen; urealisert årsslutt-regulering m/ reversal; SAF-T
  CurrencyCode/CurrencyAmount/ExchangeRate; docs/valuta.md).
- Plattform: maskin-tilgang til API-et via regnid client_credentials +
  grants (#45), PWA m/ kvitteringsfoto (#48) — M5.
- Offentlig: aksjonærregisteroppgaven + aksjebok som hendelseslogg
  (#43) — M2. Lønn + a-melding som ærlig kartlagt paraply (#46) — still
  deliberately last.

**Deliberately not planned:** kassasystem/kontantsalg — kassasystemlova
requires product certification; out of scope until a customer segment
demands it and the effort is priced.

**Regelverk over tid** (docs/regelverk.md): rules are data with
validity periods — dated satser (vat_rate is the reference pattern),
versioned vendored authority artifacts, frozen evidence formats. Gaps
tracked: satsregister m/ staleness-kontroll i revisjonsrapporten (#49),
per-inntektsår artefakter (#50), ✅ mva-terminordninger (#51 — datert
ordning per selskap m/ vedtaksreferanse, årstermin/primærnæring m/
egne frister, skattleggingsperiodeAar i meldingen; docs/mva.md),
avvikende
regnskapsår som bevisst avgrensning (#52). Yearly regelverksrevisjon is
a checklist in the doc, verifiable via #49.

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
