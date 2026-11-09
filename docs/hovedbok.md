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
- Portalen: Hovedbok-seksjonen har to dyplenkbare faner
  (Rapporter-mønsteret). **Posteringer** (`…/hovedbok`) er selve
  boken: Nytt bilag-skjemaet (kontovelger med søk i egne kontoer +
  katalogen, dimensjoner, differanse-sperre på Bokfør-knappen) og
  alle bilag med linjene sine, nyeste øverst, årsvelger,
  fritekstfilter og paginering — ALT SERVERSIDE via
  `GET /companies/{id}/vouchers?from&to&sok&limit&offset&lines=true`
  (utvidelse av det eksisterende endepunktet; uten parametre svarer
  det som før, og den lovpålagte bokføringsspesifikasjonen er urørt —
  en rapport er komplett per definisjon, dette er skjermen man blar
  i). Filteret (`sok`) leser hele bilaget: nummer, dato, tekst og
  linjenes kontonummer/-navn; filterfeltet i portalen er debouncet.
  **Kontoplan** (`…/hovedbok/kontoplan`) er indeksen, ikke
  hovedinnholdet (brukerkorreks 2026-08-06) og BEVISST ingen egen
  menyseksjon — den hører til boken sin, men har egen adresse: søk,
  vis-deaktiverte-filter, paginering, legg til fra katalog,
  egendefinert konto, navn/deaktiver. Drill-down per konto
  (`…/hovedbok/<nr>`, `/reports/kontospesifikasjon?account=` —
  filteret fantes på serveren fra #4, seksjonen tok det i bruk). Et
  bilag MED dokument hører fortsatt hjemme i innboksen; skjemaet sier
  det.

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
