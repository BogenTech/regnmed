# Utlegg og kjøregodtgjørelse

Issue #42. Ansatte og eiere legger ut privat og skal ha korrekt
refusjon — med dokumentasjonen på plass og satsene fra regelverket,
aldri fra hodet.

## Innboks-disiplinen på refusjonskrav

`expense` (migration 0026) følger bilagsinnboksens regler:

- **Innholdet er uforanderlig fra innsending.** Kvitteringen lagres
  med SHA-256 ved opplasting; trigger + kolonnegrants avviser enhver
  endring. Kjøregodtgjørelse lagrer satsene den ble beregnet med —
  en satsendring rører aldri et innsendt krav (raden er bevis).
- **Beslutninger er enveis**: innsendt → godkjent (med
  kostnadsbilaget) eller → avvist (begrunnelse påkrevd);
  godkjent → utbetalt (med utbetalingsbilaget). Ingenting
  om-besluttes, ingenting slettes — et avvist krav med begrunnelsen
  er en del av historien.
- Kvitteringen leses alltid integritetssjekket mot lagret hash.
- **Samme kvittering to ganger er ett krav, ikke to.** Innsendingen
  nekter bytes (SHA-256) som allerede står i et krav som ikke er
  avvist, og sier hvilket krav det er. Et AVVIST krav sperrer ikke:
  å sende inn på nytt med bedre forklaring er den normale veien
  tilbake. En rådgivende lås per selskap og hash
  (`pg_advisory_xact_lock`) gjør at to samtidige innsendinger av
  samme kvittering tar tur, så et dobbeltklikk ikke slipper gjennom
  (testet ved å holde låsen utenfra og se at andre innsending venter).

## Utkast: lagre nå, gjør ferdig senere

Et krav begynner som **utkast** (migration 0085). Et bilde av en
kvittering tatt på vei ut av taxien lagres med én gang — gjerne mange
på en gang — og detaljene fylles ut senere, på samme eller en annen
enhet. Et utkast er ennå ikke dokumentasjon: alle felt kan endres, og
kvitteringen kan byttes (rotert eller beskåret i nettleseren — lerret,
JPEG, maks 2400 px på lengste kant). Innboks-disiplinen begynner der den
alltid har begynt: ved **innsending**, og fra da av holder triggeren
innholdet fast. Et utkast slettes aldri; det **forkastes**, enveis.

- Utkast er bare synlige for den pengene gjelder og den som registrerte
  dem — for alle andre finnes de ikke (404), heller ikke for den som
  godkjenner, og heller ikke kvitteringen deres.
- Et utkast har ingen beslutningsspor: triggeren nekter
  `decided_by`, `voucher_id` og de andre beslutningsfeltene på et
  utkast, også skrevet rett i databasen.
- Samme kvittering i to krav avvises også når det første er et utkast;
  et avvist eller forkastet krav frigjør den.
- Innsending sier hva som mangler («velg dato», «legg ved
  kvitteringen», «oppgi hvor du kjørte fra og til») — portalen viser det
  ved feltet, serveren håndhever det, og databasens check-constraints
  nekter et innsendt krav uten dato, beløp og beskrivelse.

**På vegne av andre.** Den som håndterer utlegg (`UTLEGG_GODKJENN`) kan
registrere et krav for et aktivt medlem av selskapet
(`registered_by_person` lagres ved siden av `person_id`). Den ansatte ser
kravet og hvem som registrerte det. Med attestering slått på kan
verken den pengene gjelder eller den som registrerte kravet godkjenne
det — begge er forfattere av det.

**Dimensjoner.** Avdeling og prosjekt (koder — de er permanente) og
«viderefaktureres» lagres på kravet og føres på kostnadslinjene ved
godkjenning, ikke på mva-linjen eller gjelden til den ansatte.
Viderefakturering krever prosjekt; selve faktureringen er neste steg.

## Utlegg

Innsending: rå kvittering + dato/beløp/formål. Godkjenning i ÉN
transaksjon:

1. Bilaget: debet kostnadskonto (netto), eventuell inngående mva
   skilt ut med [`split_gross`](mva.md) etter dato-riktig sats
   (debet 2710), kredit mellomregningskonto (2910 gjeld til ansatte
   som standard) for brutto.
2. **Kvitteringen kopieres inn som vedlegg på bilaget** — 
   oppbevaringsplikten dekker originaldokumentasjonen fra det
   øyeblikket kostnaden er bokført (samme mekanikk som innboksen).
3. Den enveise statusovergangen, med mellomregningskontoen lagret så
   utbetalingen debiterer samme konto.

### Kvitteringen leses

Når et utkast har en kvittering, leser serveren den og fyller ut feltene
som fortsatt er tomme: dato, beløp og hvem som ble betalt
(`GET …/expenses/{eid}/suggestion`). En PDF leses fra tekstlaget, et
foto av tekstleseren (docs/bilagstolkning.md). Hvem som ble betalt
hentes i denne rekkefølgen: leverandøren med samme orgnr i reskontroen,
navnet i Enhetsregisteret for orgnr, ellers kvitteringens øverste linje.

Reglene:

- **Et forslag er ikke et faktum.** Ingenting lagres før brukeren
  lagrer utkastet; hvert utfylt felt sier hvorfor («Lest fra
  kvitteringen: etter «totalt»»).
- **Det brukeren skrev, overskrives aldri.** Et felt fylles bare når
  det er tomt, eller fortsatt holder forrige forslag — så en ny lesing
  etter rotering kan rette seg selv.
- **Leses på nytt hver gang, lagres aldri.** En bedre leser forbedrer
  også gamle utkast.
- Samme tilgang som kvitteringen: et utkast er bare innsenderens og
  den som registrerte det; for alle andre finnes det ikke (404).
- Mva leses, men vises ikke for innsenderen: kravet bærer én mva-kode,
  og den velges av den som godkjenner. Mva fordelt på flere satser per
  kvittering er ennå ikke bygget.

## Kjøregodtgjørelse

km × statens sats på **kjøredatoen**, fra satsregisteret
(`km_godtgjorelse`, `km_godtgjorelse_trekkfri` — øre per km, med
kilde; docs/regelverk.md). En dato utenfor registerets dekning
avvises høyt, aldri gjettet. Ren beregning i
`regnmed-core::expenses::mileage_allowance`:

- beløp = km × sats
- trekkfri del = km × trekkfri sats (aldri over beløpet)
- **trekkpliktig del** = differansen (2026: 1,80 kr/km)

Den trekkpliktige delen skal lønnsinnberettes. A-meldingen (#46)
finnes nå, men tar ennå IKKE med kjøregodtgjørelse fra utleggene, så
delen **varsles tydelig** — i skjemaet før innsending (beløpet regnes
med satsen fra `GET …/expenses/kjoring?dato=`), på kravet og i
godkjenningssvaret — med beskjed om at den må meldes særskilt. Aldri
skjult, aldri halvhåndtert.
Godkjenning posterer hele beløpet (debet 7100 bilgodtgjørelse som
standard) mot mellomregning; bilagslinjen bærer km og sats.

### Turen: strekning, kjøretøy, passasjerer

Det som gjør godtgjørelsen trekkfri er dokumentasjonen, så den er
felter, ikke fritekst: **fra** og **til**,
**tur/retur** (kilometer oppgis én vei og dobles), **kjøretøy** og
**passasjerer** med navn. Alt står på bilagslinjen ved godkjenning.

Adressefeltene foreslår mens man skriver, og tar både adresse og
firmanavn: egne adresser fra tidligere turer først (også i et tomt
felt), så selskapets egne kunder og leverandører (navn → adressen som
står på parten — bare for den som har `RESKONTRO_LES`; en ansatt som
fører en tur får ikke kunderegisteret gjennom adressefeltet), så ethvert
foretak i Enhetsregisteret ved navn (forretningsadressen — reisen trenger
ikke gå til noen man handler med), så Kartverkets åpne adresseregister
(`ws.geonorge.no/adresser`, ingen nøkkel). De tre siste spørres
samtidig via `GET …/expenses/addresses?q=` — portalen spør regnmed,
regnmed spør Enhetsregisteret og Kartverket (3 s tidsavbrudd hver).
Et valgt foretak fylles inn som «Navn, gate, postnummer sted», så
navnet står i dokumentasjonen. CSP-en forblir
`connect-src 'self'`, og brukerens IP-adresse når aldri tredjeparten.
Forslagene er en bekvemmelighet: feiler oppslaget, er svaret en tom
liste og feltet tar fri tekst som før. 
**Kjørerute og avstand.** Når både fra og til er fylt ut, foreslås
avstanden: Kartverket finner punktene (et «Firma, gate, postnummer
sted» prøves på nytt uten firmanavnet), OpenRouteService finner
kjøreruten (`regnmed-gov::routing`, `GET …/expenses/route?from=&to=`).
Avstanden fylles i kilometerfeltet bare når det er tomt eller fortsatt
har forrige forslag — aldri over et tall brukeren har skrevet, for
kilometeren er kravet til den som kjørte, og en omvei kan ha en god
grunn. Avviker de, står begge tallene på skjermen. Den som godkjenner
ser beregnet rute ved siden av krevde kilometer, og ruten på kart
(Leaflet, lastet bare når kartet åpnes; Kartverkets åpne
topografiske kart — CSP-en slipper `cache.kartverket.no` inn som
bilder og ingenting annet).

Nøkkelen er `ORS_API_KEY` (valgfri hemmelighet `ors-credentials` i
test og prod, docs/secrets.md). Uten den svarer endepunktet
`available: false`, og skjemaet ber om kilometer som før. Avstanden
lagres ikke på kravet: den regnes ut på nytt ved visning, og samme par
adresser besvares fra minnet i et døgn (per replika), så kvoten hos
OpenRouteService ikke brukes opp av den som blar i krav.

| Kjøretøy | Satsdomene | 2026, øre/km (stat / trekkfri) |
| --- | --- | --- |
| Bil, også utslippsfri | `km_godtgjorelse` | 530 / 350 |
| Motorsykkel over 125 ccm | `km_godtgjorelse_motorsykkel` | 295 / 295 |
| Moped, motorsykkel til og med 125 ccm | `km_godtgjorelse_moped` | 200 / 200 |
| Snøscooter og ATV | `km_godtgjorelse_snoscooter` | 1000 / 1000 |
| Båt med motor | `km_godtgjorelse_bat` | 750 / 750 |
| Passasjertillegg, per passasjer | `km_passasjertillegg` | 100 / 100 |

Kilder: særavtale reise i Norge 2026–2027 §§ 2–3 og Skatteetatens
satser for bilgodtgjørelse 2026 (hentet 2026-11-13; hver rad bærer sin
kilde i satsregisteret). En bil betales likt uansett drivstoff; at den
er utslippsfri lagres fordi bompengene ikke er like. Beløp =
km × (sats + passasjerer × tillegg), trekkfri del tilsvarende — ren
beregning i `regnmed-core::expenses::trip_allowance`. Tillegg for
tilhenger og skogs-/anleggsvei er ikke bygget.

### Bompenger, rushtidsavgift, ferje og parkering

Dekkes etter **faktisk kostnad**, ikke per km, og er derfor egne
utleggskrav knyttet til turen (`trip_expense_id`) — hvert med sin
kvittering eller AutoPASS-oversikt, sin mva-vurdering og sin konto ved
godkjenning. Kvitteringen kan komme senere: når turen sendes inn, går
de ferdige kostnadene med i samme transaksjon, og resten blir liggende
som utkast med beskjed om hva som mangler. Forkastes turen, forkastes
utkastene på den.

## Utbetaling

`POST …/expenses/{id}/pay`: debet mellomregningskontoen fra
godkjenningen, kredit bank (1920 som standard) — én transaksjon med
statusovergangen. Når remittering (#33) kommer, mater godkjente krav
betalingslisten i stedet for et manuelt klikk; bankavstemmingen
matcher utbetalingsbilaget mot kontoutskriften som ethvert annet
bilag.

Selvgodkjenning er tillatt som standard — et enkeltpersonforetak må
kunne gjøre alt. Slår selskapet på attestering (docs/attestering.md,
#47), kan innsenderen ikke lenger godkjenne sitt eget krav; regelen
håndheves i `approve_expense`, ikke i portalen.

## Endpoints

Utkast:

- `POST /companies/{id}/expenses/receipts?filename=&person_id=` — rå kvittering → utkast
- `POST /companies/{id}/expenses` — utkast som JSON (`kind`, `person_id`, `trip_expense_id` og feltene)
- `PUT  /companies/{id}/expenses/{eid}` — skriv om et utkast
- `PUT  /companies/{id}/expenses/{eid}/receipt?filename=` — bytt kvitteringen på et utkast
- `POST /companies/{id}/expenses/{eid}/submit` — send inn (innholdet låses)
- `POST /companies/{id}/expenses/{eid}/discard` — forkast
- `GET  /companies/{id}/expenses/claimants` — hvem det kan registreres for
- `GET  /companies/{id}/expenses?status=utkast` / `?trip={eid}` — egne utkast / kostnadene på en tur
- `GET  /companies/{id}/expenses/kjoring?dato=&vehicle=` — satsene, også passasjertillegget
- `GET  /companies/{id}/expenses/{eid}/suggestion` — det kvitteringen sier: dato, beløp, hvem, med begrunnelser

Direkte innsending (som før, for integrasjoner):

- `GET  /companies/{id}/expenses?status=&limit=&offset=` — kravene
  nyeste først, med `own`-flagg, `total` og `counts` per status.
  `status` er en FANE: `apne` (innsendt + godkjent — pengene er ikke
  ute ennå), eller en av de lagrede statusene; tom = alle. Ukjent
  status gir 400.
- `GET  /companies/{id}/expenses/{eid}` — ett krav, med begge
  bilagene (`voucher`, `utbetalt_voucher`) og hvem som besluttet
- `POST /companies/{id}/expenses/utlegg?filename=&dato=&belop_ore=&beskrivelse=` — rå kvittering som body
- `GET  /companies/{id}/expenses/kjoring?dato=` — statens km-sats og
  den trekkfrie satsen den dagen (400 utenfor satsregisterets dekning)
- `POST /companies/{id}/expenses/kjoring`
- `GET  /companies/{id}/expenses/{eid}/receipt` (hash-sjekket)
- `POST /companies/{id}/expenses/{eid}/approve` (konto/mva_kode/motkonto valgfritt)
- `POST /companies/{id}/expenses/{eid}/reject` (note påkrevd)
- `POST /companies/{id}/expenses/{eid}/pay`

Registrering krever `UTLEGG_SKRIV_EGNE` (egne krav); godkjenning og
avvisning `UTLEGG_GODKJENN`, utbetaling `UTLEGG_UTBETAL`. Den som
bare har `UTLEGG_LES_EGNE` ser sine egne krav — et annet krav svarer
404 både i listen, på kravsiden og for kvitteringen.

Portal (**Utlegg**, `#/c/{id}/utlegg/…`): kravene først, i fanene
Åpne / Utbetalt / Avvist med tellere, sidevis fra serveren. Statusen
står i ord («Venter på godkjenning», «Godkjent, venter på
utbetaling»). «Nytt utlegg eller kjøring» (`…/utlegg/ny`) er ett
skjema med merkede felt; kvitteringen velges først (kamera eller fil
på telefon), og knappen er opptatt til svaret kommer. Hvert krav har
sin side (`…/utlegg/{id}`) med kvitteringen forhåndsvist ved siden av
beslutningen (samme bytesjekk som innboksen, `lib/preview.js`),
kontovelger, mva-valg med varsel for representasjon (mval. § 8-3),
linjene som vil bokføres, og bekreftelse med dato og bankkonto før
utbetaling. Knappene vises bare for den som har retten, og med
attestering på tilbys ikke godkjenning av eget krav; etter godkjenning
eller avvisning går siden til neste krav som venter (ikke ens egne når
attestering er på).

Bevisst utenfor så langt: diett/per-diem-satser, reise som samling av
krav, KI-lesing av kvitteringen (agent-fasen), mva fordelt på flere satser
per kvittering, a-melding for godtgjørelsene og betalingslisten.

## Where it is tested

- `crates/regnmed-api/tests/grupper/expenses.rs::expense_drafts_flow` —
  utkast uten dato/beløp, usynlig og uskrivbart for andre (404),
  innsending som navngir det som mangler, ukjent prosjekt avvist,
  kvitteringsbytte og duplikatvakt mot utkast, innholdet låst fra
  innsending (API og direkte SQL), prosjektet på kostnadslinjen og ikke
  på gjelden, tur med tur/retur + utslippsfri bil + passasjer
  (63 000 øre, 18 000 trekkpliktig) og strekningen på bilagslinjen,
  bompenger med og uten kvittering ved innsending, registrering på
  vegne (403 for ansatt, 400 for ikke-medlem) og fire øyne mot den som
  registrerte (vakten sett feile), forkasting enveis og med utkastene
  på turen.
- `crates/regnmed-api/tests/grupper/expenses.rs::a_trip_route_is_found_from_two_addresses`
  — uten nøkkel `available: false`; med lokale stand-ins for Kartverket
  og OpenRouteService: firmanavnet droppes til gaten treffer (vakten
  sett feile), [lon, lat] snus til [lat, lon], ukjent adresse navngis.
  Formen på et ekte OpenRouteService-svar er ennå ikke sjekket mot en
  levende nøkkel.
- `crates/regnmed-api/tests/grupper/expenses.rs::a_receipt_photo_prefills_the_draft`
  — uten tekstleser `source: "ingen"`; med en lokal stand-in for
  tekstleseren og Enhetsregisteret: dato, beløp etter «totalt» og
  registerets navn framfor OCR-ens; en kollega får 404; en tekstleser
  som feiler gir en advarsel, ikke en feil.
- `regnmed-core/src/expenses.rs` — passasjertillegg på begge sider,
  kjøretøy → satsdomener.

- `regnmed-core/src/expenses.rs` — split trekkfri/trekkpliktig,
  historisk sats-invertering, null-tilfeller.
- `crates/regnmed-api/tests/grupper/expenses.rs` — end to end: kvittering
  rundtur + DB-immutabilitet, samme kvittering avvist som duplikat,
  manglende km sagt i en setning, km-satsen for skjemaet, fanene og
  tellerne, kravsiden med begge bilag, godkjenning med mva-splitt (7790/2710/
  2910-saldoer verifisert) og vedlegget på bilaget, enveis
  beslutninger (også ulovlige overganger direkte i SQL), kjøring med
  trekkpliktig-varsel og høy feil utenfor satsdekning, avvisning
  krever begrunnelse, utbetaling mot bank, kjedeverifikasjon.
- `crates/regnmed-api/tests/grupper/access.rs`
  (`an_employee_may_do_their_own`) — ansatt åpner sitt eget krav,
  får 404 på en kollegas.
- `ui/portal/src/sections/utlegg/expenseMath.test.js` — skjemaets
  kjøregodtgjørelse er kjernens (`mileage_allowance`), km-tolkning;
  `ui/portal/src/lib/preview.test.js` — typen avgjøres av bytene.
