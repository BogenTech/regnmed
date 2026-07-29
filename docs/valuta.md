# Flervaluta

Issue #44. Bokføringsvalutaen er NOK — hovedboken fører aldri annet.
Dokumenter bærer transaksjonsvalutaen, og posteringslinjene bærer
**hva transaksjonen lød på og kursen den ble bokført til**, som bevis.

## Representasjon: heltall hele veien

- Valutabeløp i valutaens **minste enhet** (cent); fortegn som
  NOK-beløpet.
- Kurs i **mikro-NOK per valutaenhet** (11,6543 kr/EUR → 11 654 300).
- Omregning: `round_half_away(cent × kurs / 10⁶)` øre
  (`regnmed-core::valuta::nok_ore`). Summer er summer av avrundede
  deler — aldri omvendt.

## Hash format v4

Valutainformasjonen på en linje (kode, beløp, kurs) er en del av den
kanoniske serialiseringen — å omskrive hva en transaksjon lød på,
eller kursen den ble bokført til, bryter kjeden som all annen
manipulasjon. v1–v3 verifiserer for alltid; golden-testene pinner alle
fire digestene (`regnmed-core::hash`, docs/ledger.md).

Ved postering er NOK-beløpet autoritativt, men det sanitets-sjekkes
mot `cent × kurs` innenfor 1 kr — større avvik er en enhetsfeil (kurs
forskjøvet en tierpotens), ikke avrunding, og avvises.

## Valutakurser: markedsdata med kilde

Global datert tabell `valutakurs` (migration 0027): én kurs per
(valuta, dato), append-only, **kilde på hver rad**. Oppslaget er
"siste notering på eller før datoen" (Norges Bank noterer bankdager);
en dato før dekningen feiler høyt.

Kilder:

- **Norges Banks åpne API** (`regnmed-gov::norgesbank`): SDMX-JSON fra
  `data.norges-bank.no`, parser testet mot et vendored eksempel
  (docs/valuta/norges-bank-exr-sample.json). To feller håndteres
  eksplisitt: `UNIT_MULT` (SEK/DKK/JPY noteres per 100) og at verdier
  parses som desimalstrenger — flyttall rører aldri kurser.
  `regnmed fetch-rates --currencies EUR,USD,SEK` (CLI) og
  `POST …/currency/rates/fetch` (portal-knappen) er samme kodevei.
- **Manuelt**: `POST …/currency/rates` — alltid mulig, registrator
  blir kilde.

## Faktura i valuta

`InvoiceDraft.valuta`: linjebeløpene er i valutaens cent; kursen på
**fakturadatoen** hentes fra registeret (feiler høyt utenfor
dekning). Hver linje omregnes til NOK for posteringen; fordringen er
den eksakte summen av de omregnede delene (bilaget balanserer per
konstruksjon), og alle linjene bærer valutainformasjonen. PDF-en
viser dokumentbeløpene med «Alle beløp i EUR. Motverdi NOK …».
Kreditnota reverserer til **originalkursen**, så NOK-siden nulles
eksakt — en korreksjon skaper aldri falsk agio.

## Realisert agio ved oppgjør

`match_valuta` (`POST …/reskontro/matches` med `valuta_cent`): begge
posteringene må være i samme valuta; NOK-forbruket per side er
proporsjonalt med **linjens egen bokførte relasjon** (beløp/valutabeløp)
— ingen ekstern kurs inn i oppgjøret. Differansen posteres som
ordinært bilag (8060 valutagevinst / 8160 valutatap) med en
party-bærende overføringslinje på reskontrokontoen, og en andre
match-rad lukker overføringen — **i samme transaksjon** som matchen.
Begge sidenes NOK-rester når eksakt null; åpen valutarest =
valutabeløp − SUM(matchede cent), beregnet som alt annet.

## Urealisert kursregulering

`POST …/currency/regulate {dato, balansekonto}`: alle åpne
valutaposter reprises til siste kurs på datoen; nettodifferansen
posteres på datoen (balansekonto mot 8060/8160) og **reverseres dagen
etter** (`reverses`-lenket) — i én transaksjon. Selv-ryddende:
realisert agio ved senere oppgjør påvirkes ikke. Kjør én gang per
årsslutt; endepunktet er ikke idempotent (et nytt kall posterer et
nytt par), hvilket er synlig i hovedboken — aldri skjult.

## SAF-T

Linjer med valuta eksporterer `CurrencyCode`/`CurrencyAmount`/
`ExchangeRate` inne i Debit-/CreditAmount-strukturen, XSD-validert
som resten av eksporten.

## Bevisste avgrensninger

- **Valutakontoer i bank** (bankkonto ført i valuta) er utenfor v1 —
  NOK-bank først (issuen). Innbetalinger i valuta posteres med
  valutainformasjon på linjene, som i testen.
- Sikringsbokføring er utenfor scope.
- Purringer på valutafakturaer renderer i NOK (rest-NOK er
  purregrunnlaget).

## Where it is tested

- `regnmed-core/src/valuta.rs` — omregning, kursparsing, koder.
- `regnmed-core/src/hash.rs` — golden v4 + tampering på valutafeltene.
- `regnmed-gov/src/norgesbank.rs` — SDMX-parseren mot vendored sample
  (inkl. UNIT_MULT).
- `crates/regnmed-api/tests/grupper/valuta.rs` — end to end: kurser (manuell +
  avvist søppel), EUR-faktura med hash-dekket valutainformasjon og
  NOK-postering, enhetsfeil avvist ved postering, valutamatch med
  agio i samme transaksjon (reskontro eksakt i null), urealisert
  regulering med reversal, kjedeverifikasjon (v4) og XSD-validert
  SAF-T med valutafelter.
