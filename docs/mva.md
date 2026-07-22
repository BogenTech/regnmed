# Merverdiavgift: codes, rates, spesifikasjon, melding

## Standard codes

regnmed uses **Skatteetaten's SAF-T standard VAT codes directly** as its
own codes (`vat_code` table, complete list per
`docs/saft/Standard_Tax_Codes.csv`). There is no internal→standard
mapping to maintain or get wrong; SAF-T's `StandardTaxCode` and the
mva-melding's `mvaKode` are our codes verbatim.

Every code has a **rate class**: `regular` (25 %), `middle` (15 %,
næringsmidler), `low` (12 %), `raw_fish` (11,11 %), `zero`. A code is a
stable identity; a rate is a dated fact.

## Dated rates

`vat_rate` (migration 0006) stores rates in **basis points** with
validity dates: e.g. lav sats 10 % from 2016, 12 % from 2018-01-01, 6 %
during covid (2020-04-01 → 2021-09-30), 12 % since. All beregning —
reports, SAF-T tax lines, mva-melding — resolves the rate **valid on the
voucher date**. `vat_code.rate_percent` is informational only ("current
rate") and is never used in computation. History before 2016 is out of
scope; a voucher older than the rate table fails loudly rather than
computing wrongly.

## Beregning rules

In `regnmed-core::mva` (pure, no I/O):

- `vat_of_base(base_ore, rate_bp)` — VAT from grunnlag, integer øre,
  rounded half away from zero, sign-preserving.
- `split_gross(gross_ore, rate_bp)` — splits a VAT-inclusive amount so
  that `base + vat == gross` exactly (vat is the remainder).
- Terminer are the standard two-month periods (1 = januar–februar … 6 =
  november–desember); `Termin::of/start/end` own the boundary math.

Ledger sign convention throughout: positive = debit. Sales bases are
credits (negative), purchase bases debits (positive).

## Mva-spesifikasjon (`regnmed mva-report`)

Per termin (or year): for each code and rate actually used, the summed
grunnlag and the **beregnet** avgift (`vat_of_base` on the sum). Beregnet
— not posted — because comparing it against the balance of the posted
VAT accounts (2700/2710) is precisely the accountant's control. A period
spanning a rate change shows one line per rate. The summary derives
utgående (codes 3, 31, 32, 33), inngående fradrag (1, 11, 12, 13, 14,
15) and netto å betale / til gode.

## Mva-melding (`regnmed mva-melding`)

Built in `regnmed-core::mvamelding` from the same spesifikasjon lines,
rendered as `mvaMeldingDto` XML per Skatteetaten's published XSD
(vendored in `docs/mva-melding/`, validated on every test run and in CI).

Conversion rules — all in one place, tested:

| Ledger | Melding |
| --- | --- |
| signed øre, positive = debit | **whole kroner**, rounded half away from zero |
| utgående avgift is a credit (negative) | signed by effect on payable: utgående **positive**, fradrag **negative** |
| grunnlag on every coded line | grunnlag + sats only on utgående/omsetning codes; fradrag codes report only `merverdiavgift` |
| code 0 postings exist | code 0 is **not reported** in the melding |

`fastsattMerverdiavgift` = the sum of all line effects, which is the same
netto the mva-report shows.

**Known limitation** (documented, deliberate): import/omvendt
avgiftsplikt codes (2x, 8x, 9x) are emitted with their beregnet side
only; the full two-sided treatment (utgående + fradrag from one posting)
is finalized together with real submission testing against Skatteetaten's
test environment.

Validation and submission against Skatteetaten's APIs: see
[gov.md](gov.md).

## Web API (the product surface)

The web is the product; the CLI wraps the same crate functions for
ops/admin. Authenticated, engagement-guarded endpoints (see
[auth.md](auth.md)):

| Endpoint | Returns |
| --- | --- |
| `GET /companies/{id}/reports/mva?year=&termin=` | spesifikasjon as JSON (øre integers; the UI formats) |
| `GET /companies/{id}/reports/mva-melding?year=&termin=` | `mvaMeldingDto` XML download |
| `GET /companies/{id}/reports/saft?year=` (or `from=&to=`) | SAF-T XML download; header contact defaults to the authenticated person's name |

## Where it is tested

- `crates/regnmed-core/src/mva.rs` — termin boundaries (incl. leap
  years), historical rate lookup, rounding, gross-split exactness, code
  direction.
- `crates/regnmed-core/src/mvamelding.rs` — sign/unit conversion, code 0
  exclusion, fradrag lines without grunnlag, fastsatt sum, XSD validity.
- `crates/regnmed-db/tests/mva.rs` (real Postgres, also CI) — the
  spesifikasjon's numbers per termin, the 2017 historical rate (10 %),
  and the dated rate flowing into SAF-T lines.
