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
- **Terminordninger** (#51): to-måneder is the default; **årstermin**
  (omsetning under grensen, etter søknad) and **primærnæring** are
  yearly. The ordning Skatteetaten has GRANTED is recorded per company
  with `valid_from` (migration 0028, append-only history with the
  vedtaksreferanse) — eligibility is never auto-detected. `Terminordning`
  owns the ordning-aware periode math and the **leveringsfrister**
  (skatteforvaltningsforskriften §8-3: 1 måned og 10 dager, med
  særregelen 31. august for 3. termin; årstermin 10. mars; primærnæring
  10. april). Spesifikasjonen, mva-meldingen (skattleggingsperiodeAar
  for the yearly ordninger — the schema's own distinction) and the
  portal picker all follow the company's ordning; a periode number
  outside it is refused. Kortere terminer ved restanse are pålagt
  individually and deliberately out of scope until asked.

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

### Omvendt avgiftsplikt og innførsel er TOSIDIG (#82)

Kjøperen både beregner utgående avgift og fører fradraget for den —
mval. §11-1 (2) og (3), jf. §3-30. Fram til 2026-08-07 sendte vi bare
den beregnede siden, og `fastsattMerverdiavgift` krevde da inn 25 % av
et grunnlag kjøperen ikke skyldte noe på.

Hvilke koder som bærer fradraget er **ikke vår vurdering**. Det står per
kode i Skatteetatens egen kodeliste (*Norwegian SAF-T Standard VAT/Tax
codes*, dokumentet mva-melding-XSD-en selv peker til for `mvaKode`).
Ordrett for kode 81: «Grunnlaget og beregnet utgående
innførselsmerverdiavgift føres i post 9, mens fradragsberettiget
inngående innførselsmerverdiavgift føres i post 17». For kode 82 stopper
den samme setningen etter post 9.

| Koder | Kodelisten sier | Virkning på fastsatt |
| --- | --- | --- |
| 81, 83, 86, 88, 91 | beregnet utgående **og** fradrag | **null** |
| 82, 84, 87, 89, 92 | bare beregnet utgående | hele avgiften |
| 14, 15 | bare fradraget (avgiften betalt ved innførsel) | fradrag |
| 20, 21, 22 | kostnadsmarkør på fakturaen | **rapporteres ikke** |

Postnumrene tilhører gamle RF-0002; tosidigheten de beskriver er en
egenskap ved KODEN og følger med inn i den kodebaserte meldingen.

Kostnadsmarkørene var tidligere rutet sammen med 8x/9x-kodene, slik at
en innførsel beregnet avgift to ganger — én gang under kode 21 og én
gang under kode 81. De hoppes nå over som kode 0. Kodelisten sier selv
at de ikke er obligatoriske, og at «ved selve avgiftsberegningen
benyttes kode 81 eller kode 14».

**Gjenstår, uavklart mot Skatteetaten:** om en tosidig kode skal sendes
som ÉN linje (beregnet avgift i `merverdiavgift`, fradraget bare i
totalen — slik vi gjør nå) eller som to linjer. XSD-en tillater begge,
og `fastsattMerverdiavgift` er uansett riktig. Spørsmålet avklares mot
valideringstjenesten før første innsending (`regnmed mva-melding
--validate`, docs/gov.md). Selve avgiftsbeløpet er ikke lenger avhengig
av svaret — bare linjeformen.

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
