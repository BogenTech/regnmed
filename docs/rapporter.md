# Lovpålagte spesifikasjoner og rapporter

Bokføringsforskriften §3-1 requires that a set of specifications can be
produced from the books for any period, on demand, for as long as the
oppbevaringsplikt runs. regnmed satisfies this the only way that cannot
drift: **every report is a query over the immutable ledger** — pure
`SUM(amount_ore)` and ordered SELECTs, never stored report state. A
report for 2026 produced today and one produced in five years are the
same numbers, because the underlying vouchers cannot change
([ledger.md](ledger.md)).

The specifications and where they live:

| Spesifikasjon | Endpoint | Source |
| --- | --- | --- |
| Bokføringsspesifikasjon | `GET /companies/{id}/reports/bokforingsspesifikasjon?from=&to=` | every voucher in posting (chain) order with all lines |
| Kontospesifikasjon (hovedbok) | `GET …/reports/kontospesifikasjon?from=&to=[&account=]` | every posting per account, running saldo seeded from inngående balance, bilagshenvisning `journal-år-nummer` |
| Kunde-/leverandørspesifikasjon | `GET …/parties/{pid}/items` | reskontro ([reskontro.md](reskontro.md)) |
| Mva-spesifikasjon | `GET …/reports/mva?year=&termin=` | dated rates ([mva.md](mva.md)) |
| Saldobalanse | `GET …/reports/saldobalanse?from=&to=` | per account: inngående, debet, kredit, utgående |
| Resultatregnskap | `GET …/reports/resultat?from=&to=` (optional `avdeling=`/`prosjekt=` — resultat per dimensjon, docs/dimensjoner.md) | NS 4102 classes 3–8 |
| Balanse | `GET …/reports/balanse?date=` | NS 4102 classes 1–2 + udisponert resultat |

All endpoints follow the standard guard: any access level may read
(reports never mutate; revisor read access is the point), no access →
404. The portal's **Rapporter** section renders all of them per year.

## Sign conventions

The ledger stores debit positive, credit negative
([docs/README.md](README.md)). Saldobalanse, kontospesifikasjon and
bokføringsspesifikasjon show **ledger signs** unchanged — they are the
accountant's working documents. Resultat and balanse are presentation:
inntekter (class 3) and egenkapital/gjeld (class 2) are shown negated so
the reader sees positive numbers; the grouping logic is pure and unit
tested in `regnmed-core::regnskap`.

## Resultat/balanse grouping (NS 4102 classes)

1 eiendeler · 2 egenkapital og gjeld · 3 driftsinntekter ·
4 varekostnad · 5 lønnskostnad · 6–7 annen driftskostnad ·
8 finansposter, skatt m.m.

Derived lines: driftsresultat `= −(sum classes 3–7)`, årsresultat
`= −(sum classes 3–8)`. The balanse shows the running result as
**udisponert resultat** on the egenkapital side, so it balances mid-year
without a closing entry; `differanse_ore` (eiendeler − egenkapital/gjeld
− udisponert) is exposed in the API and must be zero — double-entry
guarantees it, and the integration test asserts it.

This is the *internal* statutory presentation. The formal årsregnskap
oppstillingsplan (regnskapsloven) and skattemelding/næringsspesifikasjon
are separate M2 deliverables (issues #11, #12) that will build on the
same saldo queries.

## Where it is tested

- `crates/regnmed-core/src/regnskap.rs` — grouping, presentation signs,
  driftsresultat/årsresultat, the balanse identity, class 6+7 merge,
  zero-balance omission.
- `crates/regnmed-api/tests/grupper/regnskap.rs` (real Postgres, also in CI) —
  saldobalanse carries inngående across a year boundary and splits
  debet/kredit; kontospesifikasjon running saldo and bilagshenvisning;
  bokføringsspesifikasjon in posting order with every voucher balancing;
  resultat/balanse reconcile to the øre; 404 for outsiders, 400 for an
  inverted period.

## Nøkkeltall og likviditet (#36)

`GET /companies/{id}/reports/nokkeltall?year=` — oversiktens svar på
«hvordan går det», utelukkende fra tall som allerede finnes, som rene
SUM-spørringer (aldri lagret tilstand):

- **Resultat hittil i år** (resultatkontoene 3xxx–8xxx,
  presentasjonsfortegn) mot **samme periode i fjor** — fjoråret måles
  til samme dato, så sammenlikningen er ærlig midt i året.
- **Månedskolonner**: resultat per måned for året — portalen tegner
  dem som rene CSS-søyler (ingen grafbibliotek; frugality).
- **Likviditetsbildet**: bank og kontanter (19xx-saldoene) +
  utestående kundefordringer − leverandørgjeld − beregnet mva-netto
  for inneværende periode = «disponibelt om alt gjøres opp».
- **Kommende frister**: de neste mva-fristene etter selskapets
  terminordning (docs/mva.md) — fristbevissthet uten varsler, som
  første steg.

Året styrer «hittil»-tallene og månedskolonnene; likviditet og
frister er alltid nå. Budsjettavvik hører til
[budsjett.md](budsjett.md) (#41), ikke her — der ligger også
avviksrapporten som måler de samme resultattallene mot en navngitt,
fastsatt plan.

Testet i `crates/regnmed-api/tests/grupper/nokkeltall.rs`: resultat mot
håndregnede tall (fjorårets poster ETTER cutoff teller ikke),
månedskolonner med kostnadsmåned, likviditetsbildet fra reskontro og
bank, og at fristene aldri ligger i fortiden.
