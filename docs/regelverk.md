# Regelverk som data

Norwegian accounting rules change: satser at nyttår, code lists per
inntektsår, schemas per version. regnmed's doctrine is that **rules are
data with validity periods — never code branches on a year**:

1. **Dated tables** for everything with a sats: the row says what the
   rate is and *from when*; the lookup is always "the rate valid on the
   voucher's date". A rule change is one INSERT — history stays intact,
   old periods re-report identically forever. Reference implementation:
   `vat_rate` (basis points, history back to 2016 including the
   covid-era lav-sats change; SAF-T and mva-spesifikasjon pick the rate
   per voucher date).
2. **Versioned vendored artifacts** for authority-owned formats: XSDs
   and code lists live in `docs/` next to the document explaining them,
   validated against in tests and CI. New version → new vendored file →
   conscious commit; old data keeps validating against the version that
   governed it.
3. **Frozen serialization formats** where evidence depends on them:
   hash formats and anchor formats are versioned, never edited
   (docs/ledger.md, docs/anchoring.md).

## Inventory (what is rule-bound today, and where)

| Rule | Mechanism | Location |
| --- | --- | --- |
| Mva-satser (alle klasser) | dated table | migration 0006 `vat_rate` |
| Mva-koder | standard SAF-T code list | migration 0006 `vat_code` |
| Terminer (2-mnd) | pure logic | `regnmed-core::mva::Termin` |
| Næringsspesifikasjon grouping | vendored CSV, **pinned inntektsår 2025-2026** | `crates/regnmed-core/src/saft/…csv` + docs/saft/ |
| SAF-T Financial schema | vendored XSD (v1.30) | docs/saft/ |
| Mva-melding schema | vendored XSD | docs/mva-melding/ |
| Kontonavn NS 4102 | same vendored CSV | (as grouping) |

Planned rules follow the same doctrine (their issues say so):
forsinkelsesrente og purregebyr (#29), statens km-sats (#42),
avskrivningssaldogrupper (#40), feriepenge- og aga-satser (#46),
valutakurser (#44 — dated, from Norges Bank).

## Årlig regelverksrevisjon (before each nyttår)

A recurring checklist, done as one reviewed commit in December:

1. Statsbudsjettet: mva-satser endret? → INSERT into `vat_rate`.
2. Skatteetaten: ny næringsspesifikasjon for kommende inntektsår? →
   vendor the new CSV (per-year selection: issue #50).
3. SAF-T / mva-melding schema versions unchanged? → if new, vendor and
   wire per-period selection.
4. Alle dated tables: newest `valid_from` still correct for the new
   year? (The satsregister, #49, will surface this automatically.)
5. Frister (mva-terminer, a-melding when relevant) unchanged?

Sources to watch: Skatteetatens API-dokumentasjon og SAF-T-sider,
statsbudsjettet (regjeringen.no), lovdata endringslover for bokførings-
og regnskapsloven.

## Open gaps (tracked)

- #49 satsregister: one generalized dated-rate mechanism + staleness
  warning in the revisjonsrapport.
- #50 per-inntektsår authority artifacts (grouping list chosen by the
  exported year, not by what happens to be vendored).
- #51 mva-terminordninger beyond 2-mnd (årstermin, primærnæring).
- #52 avvikende regnskapsår (non-calendar fiscal years) — conscious
  scope decision documented there.
