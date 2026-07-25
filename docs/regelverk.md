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
| Forsinkelsesrente, standardkompensasjon, inkassosats, purregebyr, statens km-satser, terskelverdier | **satsregisteret**: dated table w/ kilde per row, staleness-overvåket i revisjonsrapporten | migration 0016 `sats`, `regnmed-core::sats` |
| Saldogruppesatser a–j (sktl. §14-43) | satsregisteret (`saldogruppe_*`, bp) — lovfestet, endres sjelden, unntatt kadens som tersklene | migration 0025, consumed by `regnmed-db::asset::saldo_rapport` |
| Valutakurser | global datert tabell m/ kilde per rad, matet fra Norges Banks åpne API eller manuelt | migration 0027 `valutakurs`, `regnmed-gov::norgesbank` (docs/valuta.md) |
| Mva-koder | standard SAF-T code list | migration 0006 `vat_code` |
| Terminer (2-mnd) | pure logic | `regnmed-core::mva::Termin` |
| Næringsspesifikasjon grouping | vendored CSV **per inntektsår**, selected by the exported year, loud failure outside coverage | `regnmed-core::saft` ARGANGER + docs/saft/ |
| SAF-T Financial schema | vendored XSD (v1.30) | docs/saft/ |
| Mva-melding schema | vendored XSD | docs/mva-melding/ |
| Kontonavn NS 4102 | same vendored CSV | (as grouping) |

Live consumers: purring (#29, shipped — forsinkelsesrente segmented
per satsperiode, purregebyr-/standardkompensasjonstak per sending
date; docs/purring.md); anleggsregisteret (#40, shipped —
saldogruppesatser per år + aktiveringsgrensen ved registrering;
docs/anlegg.md); kjøregodtgjørelse (#42, shipped — statens sats og
trekkfri sats på kjøredatoen, lagret på kravet ved innsending;
docs/utlegg.md); flervaluta (#44, shipped — daterte kurser fra
Norges Bank, kursen på bilagsdatoen hash-dekket på linjen;
docs/valuta.md). Planned rules follow the same doctrine (their
issues say so): feriepenge- og aga-satser (#46).

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

- ✅ #49 satsregister: shipped — dated `sats` table seeded with
  verified values (forsinkelsesrente H1-2025→H2-2026,
  standardkompensasjon, inkassosats/purregebyr, km-satser, terskler),
  each row carrying its legal kilde; `sats_on` lookup mirrors
  `rate_on`; the revisjonsrapport's "Regelverkssatser" kontroll flags
  any monitored domain older than its change cadence. Consumers: #29,
  #40, #42, #46.
- ✅ #50 per-inntektsår authority artifacts: shipped — the
  næringsspesifikasjon code list is a registry of vendored vintages
  selected by the inntektsår being exported (2025-2026 today); a year
  without a covering list fails loudly naming what is vendored (test-
  pinned), the CLI reports which vintage governed an export, and the
  kontoplan wizard suggests from the newest vintage. Adding a year =
  vendor the CSV + one registry entry (the December checklist step).
- ✅ #51 mva-terminordninger: shipped — dated `mva_terminordning` per
  company (granted by Skatteetaten, recorded with vedtaksreferanse,
  never inferred); spesifikasjon, melding
  (skattleggingsperiodeAar) and frister follow the ordning
  (docs/mva.md).
- 📌 #52 avvikende regnskapsår: **bevisst utsatt, ikke oversett** — se
  seksjonen under. Antakelsen er samlet i én navngitt funksjon, og
  kostnaden ved å endre den er kartlagt med fil og linje.

## Regnskapsåret er kalenderåret (#52)

Dette er den ene regelverksantakelsen regnmed gjør uten å spørre, og
den fortjener å stå skrevet:

> **regnmed antar at regnskapsåret er kalenderåret.** Onboarding spør
> ikke, og det finnes ingen innstilling.

Hjemmelen for hovedregelen er regnskapsloven §1-7: regnskapsåret er
kalenderåret. Loven åpner for **avvikende regnskapsår** i definerte
tilfeller — sesongvirksomhet og konsern med utenlandsk morselskap er
de vanlige. Målgruppen (norsk SMB) er overveldende kalenderår, så
støtten er ikke prioritert. Men en antakelse ingen kan finne igjen er
en felle, så den er behandlet slik:

**Definisjonen bor ett sted.** `regnmed-core::regnskapsar` har
`regnskapsar(dato)` og `regnskapsar_periode(år)` med enhetstester som
fester dagens oppførsel. Posteringen (bilagsnummerserien) og SAF-T-ens
`year=`-form går gjennom den. Testen
`regnskapsaret_er_kalenderaret` er der for at en endring skal være en
beslutning, ikke et uhell.

### Hva som må endres den dagen en kunde har avvikende år

Kartet er verifisert mot koden slik den står i dag:

| Sted | Hva | Status |
| --- | --- | --- |
| `regnmed-core/src/regnskapsar.rs` | Selve definisjonen + periodegrensene | **sømstedet** |
| `regnmed-db/src/ledger.rs` (post_voucher_in) | `fiscal_year` på bilaget = nummerserien per journal+år | går gjennom sømmen |
| `regnmed-api/src/reports.rs` (saft_export) | `year=` → periode | går gjennom sømmen |
| `regnmed-db/src/budsjett.rs` | `extract(year from voucher_date) = $2` i to spørringer (avvik + «fra fjoråret») | gjenstår — SQL-side |
| `regnmed-db/src/regnskap.rs` (nokkeltall) | Årets måneder + «samme dato i fjor» | gjenstår — SQL-side |
| `regnmed-db/src/asset.rs` (saldo_rapport) | Skattemessig saldo per år | gjenstår, men se under |
| Portalen | Årvelgerne viser «2026» | gjenstår — visning |

I tillegg må et selskap med avvikende år få **en datert
regnskapsårsdefinisjon** (start-måned, endres bare på en årsgrense) —
samme mønster som `mva_terminordning` (#51): registrert, ikke utledet.

### Hva som IKKE skal følge etter

To ting er kalenderforankret uansett hva regnskapsåret er, og å la dem
følge et avvikende år ville vært feil:

- **Mva-terminene** (mval. §15-1 jf. sktfvf. §8-3). De følger
  kalenderåret for alle. `regnmed-core::mva` henter aldri perioden sin
  fra regnskapsåret, og skal ikke begynne med det.
- **Skattemessige saldoavskrivninger** følger inntektsåret (sktl.
  §14-40 flg.). For selskaper med avvikende regnskapsår er inntektsåret
  det regnskapsåret som avsluttes i kalenderåret — en egen regel, ikke
  den samme variabelen.

Periodelåsing, avskrivningsbilag og bankavstemming er
måneds- eller datobaserte og berøres ikke.
