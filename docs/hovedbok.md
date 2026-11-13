# Hovedbok: kontoplan og manuell bilagsføring

Hovedbok-seksjonen er stedet der regnskapet kan SES og FØRES direkte:
kontoplanen med beregnede saldoer, drill-down per konto, og manuelle
bilag. Alt annet i systemet bokfører som bivirkning av et
forretningsdokument (faktura, innboks, lønn, utlegg …) — dette er den
frie veien, med de samme vaktene.

## Kontoplanen

- `GET /companies/{id}/accounts` (BilagLes) — selskapets kontoer med
  navn, aktiv-flagg, reskontro-merke og BEREGNET saldo/antall
  posteringer (fra månedssummene, se «Lesemodellen»), PLUSS standardkatalogen.
- **Standardkatalogen** er Skatteetatens kontoliste (254 kontoer,
  1000–8800, vendored i `regnmed-core::saft` — samme navneliste
  SAF-T-veiviseren matcher mot). Den følger med i svaret og vises i
  portalen, men SÅS ALDRI inn i selskapet: en konto blir selskapets
  første gang noen legger den til eller bokfører på den
  («catalog + picker», brukerbeslutning 2026-08-06). Saldobalansen
  holder seg dermed ren, mens hver kode en regnskapsfører kan utenat
  er ett tastetrykk unna.
- `POST /companies/{id}/accounts` (BilagBokfor) — legg til konto.
  Uten navn slås standardnavnet opp; et nummer utenfor katalogen er en
  EGENDEFINERT konto og krever eget navn. Begge er likestilte etterpå.
- `PUT /companies/{id}/accounts/{nr}` (BilagBokfor) — navn og
  aktiv-flagg er redigerbare stamdata. NUMMERET er permanent: det er
  det posteringene og hasjkjeden refererer. Deaktivering stopper NYE
  posteringer (post_voucher krever aktiv konto); historikken består,
  ingenting slettes.

## Manuell bilagsføring

- `POST /companies/{id}/vouchers` (BilagBokfor) — samme
  forespørselsform som innboks-bokføringen, uten dokument. Alt
  `post_voucher` håndhever gjelder uendret: periodelås, aktive
  kontoer, dimensjoner, reskontro-krav, dobbelt bokholderi (og
  DB-triggerne sjekker balansen uavhengig ved commit).
- **Attesteringsgrensen er lukket:** har selskapet en aktiv
  attesteringspolicy og bilaget når beløpsgrensen, NEKTES manuell
  bokføring med beskjed om å bruke innboksen — attestering er knyttet
  til innboksdokumenter (#47), og en manuell sidedør ville opphevet
  internkontrollen. Fail-closed, testfestet.
- Portalen: Hovedbok-seksjonen har fanene **Bilag** (`…/hovedbok`),
  **Nytt bilag** (`…/ny`), **Kontoplan** (`…/kontoplan`),
  **Periodisering** og **Kassaoppgjør**. *Bilag* er selve boken: alle
  bilag med linjene sine (debet og kredit), nyeste øverst, årsvelger,
  fritekstfilter og «bare uten vedlegg», ALT SERVERSIDE via
  `GET /companies/{id}/vouchers?from&to&sok&limit&offset&lines=true`
  (den lovpålagte bokføringsspesifikasjonen er urørt — en rapport er
  komplett per definisjon, dette er skjermen man blar i). Søk og år
  står i adressen. Hvert bilag har sin side (`…/hovedbok/{id}`):
  linjene, vedleggene (last ned, legg ved et manglende dokument) og
  tilbakeføringen. *Nytt bilag* har debet- og kreditkolonner, og en
  mva-kode på en linje gjør beløpet til BRUTTO: mvaen skilles ut på
  2710 (inngående) eller 2700 (utgående) med koden på nettolinjen,
  samme form som Bilag-skjemaet og samme avrunding som kjernen
  (`voucherMath.js`, testet). Linjene som bokføres vises før
  bokføring, en ukjent konto sies på linjen, og knappen er opptatt til
  svaret kommer — et bokført bilag slettes aldri, så ett trykk er ett
  bilag. Med aktiv attestering sier skjemaet på forhånd at bilag over
  grensen må gå via innboksen. *Kontoplan* er indeksen: søk (også i
  standardkontoplanen), deaktiverte, legg til fra katalog, egendefinert
  konto, navn/deaktiver. Per konto (`…/hovedbok/<nr>?ar=`):
  posteringene for året fra inngående saldo med løpende saldo, hver
  lenket til bilaget (`voucher_id` i kontospesifikasjonen). Et bilag
  MED dokument hører fortsatt hjemme i innboksen; skjemaet sier det.
- **Alle beløpsfelt leses med `oreOrNull`** (`lib/format.js`):
  «12 500,00» er tolv og et halvt tusen. Kasse- og
  periodiseringsskjemaene brukte `parseFloat`, som stopper ved første
  mellomrom — en Z-rapport på «12 500,00» ble 12 kr på begge sider,
  gikk derfor «opp», og ville blitt bokført.

## Retting: tilbakeføring

Et bokført bilag endres aldri (bokføringsloven § 9). Det rettes med en
**tilbakeføring**: `POST /companies/{id}/vouchers/{vid}/reverse`
`{dato?}` (`BILAG_BOKFOR`) bokfører et nytt bilag der HVER linje er
speilvendt — part, dimensjoner og valuta med — knyttet til originalen
gjennom `reverses_voucher_id`, som hashen dekker. I SAMME transaksjon
avstemmes hver reskontrolinje mot speilbildet for det som sto åpent, så
en tilbakeført faktura eller betaling ikke blir liggende som åpen post
(var originalen allerede betalt, står speilbildet åpent som kundens
tilgode, slik det skal). Deretter føres det riktige bilaget.

- Nektes for et bilag som allerede er tilbakeført, og for en
  tilbakeføring (før det riktige bilaget på nytt i stedet).
- Nektes for et bilag en annen flyt eier, med beskjed om hvor det
  rettes: faktura (kreditnota), purring, utlegg, betalingsliste,
  anlegg og avskrivning, lønn, aksjeeierbok, periodisering,
  årsavslutning og importert historikk (`owned_by`). En tilbakeføring
  her ville latt fakturaen stå «utstedt» uten noe bak seg.
- Datoen må ligge i en åpen periode; en låst periode avvises av
  posteringen selv.
- En valutapost lukkes også i valuta (`valuta_cent` på avstemmingen),
  ellers ville valutaresten stått åpen. Bare en uberørt valutapost kan
  lukkes slik; en delvis avstemt løses opp under Reskontro først.
- Var originalen koblet til en banklinje, fjernes koblingen i samme
  transaksjon (`bank_unmatched` i svaret): banklinjen har ikke lenger
  en postering bak seg og avstemmes på nytt mot det riktige bilaget.
- Ingen attesteringsport: en tilbakeføring tilfører ikke noe nytt, den
  opphever en postering som allerede passerte den porten som gjaldt.
- `GET /companies/{id}/vouchers/{vid}` gir bilaget med linjer (valuta
  med), antall vedlegg, `reverses`/`reversed_by` og `owner`.
- En tilbakeførings dokumentasjon er bilaget den tilbakefører.

Testet i `tests/grupper/hovedbok.rs`
(`a_voucher_is_corrected_by_a_linked_reversal`): speilvendte linjer
med part, lenket begge veier, kundens post lukket, ikke to ganger,
ikke en tilbakeføring av en tilbakeføring, importert historikk og et
utleggsbilag nektet med beskjed, en bankkobling løst og banklinjen
uavstemt igjen, kjeden verifisert.

## Lesemodellen

Rapportene leser ikke hver postering selskapet noen gang har ført. De
leser **månedssummer per konto** (`account_month_total`, migrasjon
0072): debet, kredit og antall posteringer per konto per kalendermåned.
En sum over et datointervall tar hele måneder fra summene og bare de
delvise månedene i kantene fra posteringene selv
(`regnmed-db::ledger_totals::account_sums`). Svaret er nøyaktig det
samme som `SUM(amount_ore)` over posteringene — det er hele poenget,
og det er testet mot posteringene for over to hundre intervaller,
inkludert første og siste dag i måneden og skuddårsdagen.

**Hovedboken er fortsatt fasiten.** Månedssummene er en hurtigbuffer
over den, ingen egen sannhet:

- **Holdt i takt i samme transaksjon.** En trigger på `entry`
  oppdaterer summen når posteringen settes inn, så bilaget og summen
  committes eller rulles tilbake sammen. Det finnes ingen «ikke
  oppdatert ennå» — ingen leser ser det ene uten det andre. Postering
  er allerede serialisert per selskap (kjedehodet låses), så det gir
  ingen ny ventetid; målt ~0,13 ms per posteringslinje.
- **Bare triggeren skriver.** Funksjonen er `SECURITY DEFINER`, og
  applikasjonsrollen har bare `SELECT` på tabellen — samme lagdeling
  som hovedboken selv. Posteringer kan aldri endres eller slettes, så
  en sum kan bare vokse.
- **Kontrollert mot posteringene.** `regnmed verify-ledger` (og dermed
  den ukentlige gjenopprettingskontrollen av backupen) og
  revisjonsrapportens kontroll «Rapportgrunnlag» sammenligner hver
  månedssum med summen av posteringene og navngir konto og måned som
  avviker.

Dimensjonsfiltrerte rapporter (avdeling/prosjekt) og reskontroen leser
fortsatt posteringene direkte: dimensjoner og parter er ikke i
månedssummene, og de filtrerte mengdene er små.

Målt 2026-11-09 på et selskap med 40 000 posteringer over to år:

| Spørring | Før | Etter |
|---|---|---|
| Saldobalanse for et år | 79 ms | under 1 ms |
| Kontospesifikasjon, alle kontoer, én måned | 20 s | 21 ms |
| Kontospesifikasjon, bankkontoen, ett år (10 000 linjer) | over 2 min (avbrutt) | 77 ms |

Kontospesifikasjonen var en feil, ikke bare treg: inngående saldo ble
regnet i en underspørring som Postgres kjørte én gang PER RAD over
kontoens hele historikk. Den leses nå én gang fra månedssummene.

## Verifisert mot virkeligheten (2026-08-06)

BogenTechs faktiske Conta-eksport (SAF-T Financial 1.3, regnskapsåret
2025) ble importert i ett stykke — 14 kontoer, 2 leverandører, 10
bilag, ingen advarsler — og saldobalansen stemte PÅ ØRET mot det
GODKJENTE årsregnskapet (RR-0002 fastsatt 30.06.2026): bank 1 117,75,
aksjer 100 000, utlån 150 000, egenkapital 21 108, kortsiktig gjeld
130 009,85, årsresultat −2 892,10. `verify-ledger` går kjeden fra
genesis. Funn samtidig: Conta eksporterer ÉN fil per regnskapsår, og
importen krever tom hovedbok — flerårig historikk over flere filer er
filet som oppfølger.
