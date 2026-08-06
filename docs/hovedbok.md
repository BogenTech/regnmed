# Hovedbok: kontoplan og manuell bilagsføring

Hovedbok-seksjonen er stedet der regnskapet kan SES og FØRES direkte:
kontoplanen med beregnede saldoer, drill-down per konto, og manuelle
bilag. Alt annet i systemet bokfører som bivirkning av et
forretningsdokument (faktura, innboks, lønn, utlegg …) — dette er den
frie veien, med de samme vaktene.

## Kontoplanen

- `GET /companies/{id}/accounts` (BilagLes) — selskapets kontoer med
  navn, aktiv-flagg, reskontro-merke og BEREGNET saldo/antall
  posteringer (SUM over entry, aldri lagret), PLUSS standardkatalogen.
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
  alle bilag med linjene sine (bokføringsspesifikasjonen, nyeste
  øverst, årsvelger, fritekstfilter over bilagsnr/dato/tekst/konto,
  klientside-paginering — serverside kan komme når volum krever det).
  **Kontoplan** (`…/hovedbok/kontoplan`) er indeksen, ikke
  hovedinnholdet (brukerkorreks 2026-08-06) og BEVISST ingen egen
  menyseksjon — den hører til boken sin, men har egen adresse: søk,
  vis-deaktiverte-filter, paginering, legg til fra katalog,
  egendefinert konto, navn/deaktiver. Drill-down per konto
  (`…/hovedbok/<nr>`, `/reports/kontospesifikasjon?account=` —
  filteret fantes på serveren fra #4, seksjonen tok det i bruk). Et
  bilag MED dokument hører fortsatt hjemme i innboksen; skjemaet sier
  det.

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
