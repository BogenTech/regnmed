# Anleggsregister og avskrivninger

Issue #40. Driftsmidler over aktiveringsgrensen (kr 30 000 og minst
3 års levetid, skatteloven §14-40 flg.) skal balanseføres og avskrives.
Ett register driver begge verdener:

- **Regnskapsmessig**: lineære månedlige avskrivninger, bokført som
  ordinære bilag i hovedboken.
- **Skattemessig**: saldoavskrivning per gruppe a–j til
  næringsspesifikasjonen, beregnet — aldri lagret.

## Registeret er bevis

`asset` (migration 0025) tillater INSERT og den enveise
avhendings-overgangen — ingenting annet. En trigger avviser enhver
annen UPDATE og alle DELETE; kolonnegrants begrenser app-rollen til
avhendingsfeltene. Bokført verdi lagres aldri: den er
`kostpris − SUM(bokførte avskrivninger)`, beregnet som alle andre
saldoer. Kontoene (balansekonto, avskrivningskonto) lagres som
kontonummer ved registrering, som på fakturalinjer.

Registrering under aktiveringsgrensen (fra satsregisteret, dato-styrt)
eller under 3 års levetid avvises ikke — frivillig aktivering er
lovlig — men svarer med en advarsel om at direkte kostnadsføring er
tillatt.

## Regnskapsmessige avskrivninger

Samme mønster som repeterende faktura: **systemarbeid med
menneskesynlig logg**.

- Planen er ren funksjon (`regnmed-core::anlegg::manedsbelop`): det
  avskrivbare beløpet (kostpris − restverdi) fordeles i faste
  månedsbeløp; siste måned tar resten, så planen summerer EKSAKT.
  Registrering krever minst 1 øre per måned, så en generert periode
  alltid har et bilag.
- Generering (`depreciate_due`) tar én transaksjon per
  driftsmiddel-måned: bilaget (debet avskrivningskonto, kredit
  balansekonto, datert månedens siste dag) og loggraden committer
  sammen. En partiell unik indeks på `(asset, period)` gjør
  dobbeltavskrivning umulig; feil (f.eks. låst periode) logges med
  årsak og blokkerer aldri resten.
- Avskrivning starter i anskaffelsesmåneden og stopper måneden
  driftsmidlet avhendes (avhendingsmåneden avskrives ikke).
- Kjøres av `regnmed depreciate` (månedlig CronJob i deploy/base,
  kjører den 1. og tar den nettopp avsluttede måneden) og av
  portal-knappen — begge samme kodevei.

## Avhending

`POST …/assets/{id}/dispose` i ÉN transaksjon: vederlaget inn på
motkonto, gjenværende bokført verdi ut av balansekonto, differansen
til gevinst (kredit 3880) eller tap (debet 7880) — kontoene kan
overstyres. Registerraden lukkes enveis i samme transaksjon. Er det
ingenting å postere (fullt avskrevet, null vederlag) settes
avhendingen uten bilag.

## Skattemessig saldoavskrivning

Satsene for saldogruppene a–j (skatteloven §14-43) er
**regelverksdata i satsregisteret** (`saldogruppe_a` …
`saldogruppe_j`, bp, med lovhjemmel som kilde) — aldri hardkodet.
Lovfestede satser endres sjelden og er unntatt
kadens-overvåkingen, som terskelverdiene (docs/regelverk.md).

`GET …/assets/saldo?year=` beregner fra bunnen av over hele
registeret, år for år fra første anskaffelse: grunnlag = inngående
saldo + årets tilganger (kostpris) − årets vederlag; avskrivning =
grunnlag × årets sats når grunnlaget er positivt; utgående ruller
videre. Rapporten viser også regnskapsmessig bokført verdi ved
årsslutt og den **midlertidige forskjellen** (bokført − skattemessig).
Et år utenfor satsregisterets dekning feiler høyt — aldri gjettet.

Bevisste avgrensninger (SMB-scope, dokumentert her):

- **Negativ saldo** (vederlag over saldoen) avskrives ikke og
  rapporteres som utgående — inntektsføring (§14-46) og gevinst- og
  tapskonto (§14-45) håndteres av regnskapsfører manuelt.
- Restsaldo under kr 15 000 (§14-47) direktefradras ikke automatisk.
- Skattemessige inngangssaldoer fra et tidligere system har ingen
  egen import ennå — registeret starter tomt (samme avgrensning som
  reskontro-flagg i SAF-T-importen).
- Nedskrivningstesting og leasing (IFRS 16) er utenfor scope.

## Endpoints

- `GET/POST /companies/{id}/assets` (write requires bokforing)
- `POST /companies/{id}/assets/depreciate` — generer forfalte måneder
- `POST /companies/{id}/assets/{aid}/dispose`
- `GET  /companies/{id}/assets/{aid}/runs` — avskrivningsloggen
- `GET  /companies/{id}/assets/saldo?year=` — skattemessig rapport

Portal: **Anlegg**-seksjon — register med beregnet bokført verdi,
"Generer avskrivninger"-knapp, avhending, avskrivningshistorikk per
driftsmiddel og saldotabellen med midlertidige forskjeller.

## Where it is tested

- `regnmed-core/src/anlegg.rs` — planen summerer eksakt, saldoår
  (grunnlag/avskrivning/utgående, negativt grunnlag, avrunding),
  gevinst/tap.
- `crates/regnmed-api/tests/grupper/assets.rs` — end to end: warning under
  grensen, 2 driftsmidler avskrevet månedlig (idempotent andre
  kjøring), immutabilitet i databasen (asset + logg), avhending med
  gevinst (3880-saldo verifisert) og utrangering med tap, avskrivning
  stopper ved avhending, saldorapporten mot håndregnede tall, høy
  feil utenfor satsdekningen, kjedeverifikasjon over alle bilagene.
