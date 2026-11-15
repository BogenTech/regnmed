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

- Planen er ren funksjon (`regnmed-core::assets::monthly_amount`): det
  avskrivbare beløpet (kostpris − restverdi) fordeles i faste
  månedsbeløp; siste måned tar resten, så planen summerer EKSAKT.
  Registrering krever minst 1 øre per måned, så en generert periode
  alltid har et bilag.
- Generering (`depreciate_due`) tar én transaksjon per
  driftsmiddel-måned: bilaget (debet avskrivningskonto, kredit
  balansekonto, datert månedens siste dag) og loggraden committer
  sammen. En partiell unik indeks på `(asset, period)` gjør
  dobbeltavskrivning umulig; feil logges med årsak og blokkerer aldri
  andre driftsmidler.
- **En måned i en låst periode innhentes første åpne dag.** Månedene
  bokføres i rekkefølge, så én låst måned stoppet tidligere hele planen
  for driftsmidlet — for alltid, siden en låst periode ikke kan
  bokføres i (bokføringsloven §9). Nå dateres bilaget dagen etter
  låsedatoen, fortsatt ett bilag per måned, og bilagsteksten sier
  «innhentet, perioden var låst». Det er rettelsen i åpen periode som
  posteringsvaktens egen feilmelding ber om. Historikken viser
  bokføringsdatoen når den avviker fra månedens siste dag.
- Loggen er innsettings-bar, så en måned som feiler på hver kjøring
  får én rad per forsøk. Historikken (`…/runs`) viser én rad per
  måned: den bokførte, ellers det siste forsøket med årsak.
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

Avhendingen er enveis, så den går i to steg:

- `?preview=true` gjør HELE avhendingen og ruller den tilbake, og
  svaret har bokført verdi, gevinst/tap og bilagslinjene
  (`linjer: [{konto, belop_ore, tekst}]`). En låst periode eller en
  manglende konto vises dermed før noe er bekreftet, og telleren
  rulles tilbake med transaksjonen, så ingen bilagsnummer brennes.
- Bekreftelsen sender `forventet_bokfort_ore` (bokført verdi
  personen så) og nektes hvis en avskrivning er bokført i mellomtiden.

Datoen kontrolleres: ikke før anskaffelsesdatoen (også en
DB-sjekk), og ikke i eller før en måned som allerede er avskrevet —
avhendingsmåneden avskrives aldri, så en slik dato ville latt
avskrivningen for måneder etter avhendingen stå i bokført verdi og
gitt feil gevinst eller tap. Motsatt nektes også en avhending når en
måned FØR avhendingsmåneden er forfalt, men ikke bokført: den ville
stått som forfalt etter avhendingen og blitt bokført mot et
driftsmiddel som alt er ute av boken.

Kontoene (også balanse- og avskrivningskontoen ved registrering) må
være aktive i kontoplanen. En konto fra standardkatalogen som
selskapet ikke har brukt ennå, legges til med standardnavnet — det er
den kontovelgeren tilbyr — mens en deaktivert eller ukjent konto
nektes med nummeret i meldingen.

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
- `POST /companies/{id}/assets/{aid}/dispose` (`?preview=true`)
- `GET  /companies/{id}/assets/{aid}/runs` — avskrivningsloggen
- `GET  /companies/{id}/assets/saldo?year=` — skattemessig rapport

Portal: **Anlegg**-seksjon (skrivetilgang styres av `ANLEGG_SKRIV`;
uten den vises bare registeret og saldoen):

- Registeret som stablede rader som passer en telefon: bokført verdi
  og hvor mye som er avskrevet, eller avhendingsdato og vederlag.
  Hver rad åpner avskrivningshistorikken eller avhendingsskjemaet på
  stedet; Esc lukker og fokus går tilbake til knappen.
- «Bokfør forfalte avskrivninger» viser hvor mange som ble bokført og
  lister hver måned som ikke ble det, med årsak.
- Avhending: dato og vederlag → «Se over» viser bokført verdi,
  gevinst/tap og bilagslinjene → «Avhend og bokfør».
- Nytt driftsmiddel: merkede felt, kontovelger for balanse- og
  avskrivningskonto, beløp lest som folk skriver dem («45 000»).
- Saldoavskrivningen har årvelger (forrige år fram til juni, siden
  næringsspesifikasjonen leveres for året før), og et år uten
  satsdekning sier fra i stedet for at kortet forsvinner.

## Where it is tested

- `regnmed-core/src/assets.rs` — planen summerer eksakt, saldoår
  (grunnlag/avskrivning/utgående, negativt grunnlag, avrunding),
  gevinst/tap.
- `crates/regnmed-api/tests/grupper/assets.rs` — end to end: warning under
  grensen, 2 driftsmidler avskrevet månedlig (idempotent andre
  kjøring), immutabilitet i databasen (asset + logg), avhending med
  gevinst (3880-saldo verifisert) og utrangering med tap, avskrivning
  stopper ved avhending, saldorapporten mot håndregnede tall, høy
  feil utenfor satsdekningen, kjedeverifikasjon over alle bilagene.
- Samme fil, `locked_months_catch_up_and_disposal_previews`: januar–mars
  i en låst periode innhentes 1. april mens april og mai får sine egne
  datoer; en måned som feiler to ganger vises én gang; avhending datert
  inn i en avskrevet måned nektes; forhåndsvisningen bokfører ingenting
  og gir bilagslinjene; avhending med en forfalt, ubokført måned foran
  nektes; utdatert bokført verdi nektes; standardkontoer
  tas i bruk mens ukjente nektes på norsk. Sett feile uten rettelsene.
- `ui/portal/src/sections/anlegg/anlegg.test.js` — skjemaet leser beløp
  og levetid og navngir feilene; kjøringsresultatet lister hver feil.
