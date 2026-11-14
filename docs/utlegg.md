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

Godkjenning posterer hele beløpet (debet 7100 bilgodtgjørelse som
standard) mot mellomregning; bilagslinjen bærer km og sats. Utbetalingen
går med lønnen — se «Utbetaling».

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

## Reise og diett

En tjenestereise er én **reiseregning** (`kind = 'reise'`, migrasjon
0087): avreise og hjemkomst med klokkeslett, reisemål, formål, og døgn
for døgn hvor den reisende var, hvor hen sov og hvilke måltider andre
betalte. Det er opplysningene skattebetalingsforskriften § 5-6-12 (3)
krever for at godtgjørelsen skal være trekkfri. Hotell, billetter og
taxi er vanlige utlegg med kvittering, og kjøring er vanlige
kjøringer — begge kan henge under reisen (`trip_expense_id`), og det
som er ferdig sendes inn sammen med den, i samme transaksjon (også en
kjørings egne bompenger). Det som mangler kvittering blir liggende som
utkast, og svaret sier hva som mangler. Forkastes reisen, forkastes
utkastene under den.

**Døgnene.** Et døgn er 24 timer regnet fra avreise (særavtalene § 2).
Med overnatting er en rest på 6 timer eller mer etter siste hele døgn
et eget, påbegynt døgn; under 6 timer gir ingenting. Holder det
påbegynte døgnet også en natt (ut kl. 18, hjemme kl. 10 to dager
senere), oppgis overnattingen, og det regnes som et døgn med
overnatting. En reise uten overnatting er én dagreise, 6 timer eller
mer og høyst et døgn. En kort reise med overnatting (ut kl. 19, hjemme
kl. 7) er ett døgn med overnatting; staten betaler da 6–12-timerssatsen,
siden 1 012 kr gjelder «over 12 timer med overnatting».
Reiseregningen skrives etter reisen: en hjemkomst fram i tid nektes
ved innsending, ellers ville lønnen betalt diett for dager som ikke er
reist. Reiser
over 28 døgn på samme sted følger egne regler og nektes høyt — de er
ikke bygget. Regelen står ett sted, `regnmed-core::travel::day_count`,
og portalen spør den (`…/reise/days`) i stedet for å telle selv.

**To sett satser møtes på hvert døgn:**

- **Trekkfritt** (satsforskriften 2026, FOR-2025-11-07-2216 §§ 2–3) er
  det som kan betales uten skattetrekk og arbeidsgiveravgift — og det er
  det **samme i Norge og i utlandet**:

  | | 2026 | Satsdomene |
  | --- | --- | --- |
  | Overnatting på hotell, per døgn | 693 kr | `diett_hotell_trekkfri` |
  | Pensjonat, brakke, hybel uten kjøkken | 400 kr | `diett_pensjonat_trekkfri` |
  | Privat, hybel med kjøkken | 107 kr | `diett_privat_trekkfri` |
  | Dagreise 6–12 timer | 200 kr | `diett_dag_6_12_trekkfri` |
  | Dagreise over 12 timer | 400 kr | `diett_dag_over_12_trekkfri` |
  | Ulegitimert nattillegg, bare innenlands | 452 kr | `nattillegg_trekkfri` |

  Det påbegynte døgnet følger satsen for siste overnattingssted (§ 2).
- **Det selskapet betaler** velges per selskap (Administrasjon →
  Firmaopplysninger, `reise_satser`): enten de samme skattefrie satsene
  (standard — da er ingenting trekkpliktig), eller **statens satser**:

  | Statens særavtale, 2026 | Norge | Utlandet |
  | --- | --- | --- |
  | Døgn med overnatting | 1 012 kr (`diett_overnatting`) | landets sats |
  | Dagreise eller påbegynt døgn, 6 til 12 timer | 397 kr (`diett_dag_6_12`) | halv landssats (under 12 t) |
  | Over 12 timer uten overnatting | 736 kr (`diett_dag_over_12`) | full landssats (fra 12 t) |
  | Nattillegg, privat overnatting | 452 kr (`nattillegg`) | finnes ikke |

  Særavtale reise innenlands §§ 9–10 og utenfor Norge § 8. At det
  påbegynte døgnet betales etter timebåndene (397/736 i Norge, halv
  eller full landssats ute) er vår lesning av «etter bokstav b eller
  c» — avtalen sier ikke mer.

  Overskuddet over den skattefrie satsen er **trekkpliktig lønn** med
  arbeidsgiveravgift. Et nattillegg over det trekkfrie beløpet er
  trekkpliktig i sin helhet, ikke bare overskuddet (Skatteetaten,
  «Trekkfritt ulegitimert nattillegg»). Valget lagres på kravet ved
  innsending, så en endring rører aldri et innsendt krav.

**Måltider andre betalte** trekker fra 20 % (frokost), 30 % (lunsj) og
50 % (middag) av satsen, hver rundet til hele kroner — begge sett.
Statens frokosttrekk gjelder ikke når nattillegget betales
(innenlands § 9 nr. 4); det trekkfrie gjør det.

**Utlandet.** Statens landssatser står i en tabell per land, og for
noen land per by (`travel_rate_abroad`, 171 rader fra særavtalens
vedlegg 2026, vendored i [docs/reise/](reise/README.md) — med
departementets «som for»-land og regionenes «øvrige»). Satsen er i
landets valuta og regnes om med Norges Banks kurs på døgnets dato
(«kursen … da reisen fant sted», § 15); en kurs eldre enn en uke nektes
med beskjed om å hente kurser. Valuta Norges Bank ikke noterer (BYN,
MMK, …) kan ikke brukes ennå. Velg landet der du var lengst i døgnet,
også når det er Norge (§ 8 nr. 5).

**Beregningen** er ren kode i `regnmed-core::travel::allowance`.
Satsene er de som gjelder på **avreisedagen** — en reise over nyttår
regnes med det gamle årets satser. Ved innsending lagres hvert døgn
(`travel_lines`): tidsrom, sted, overnatting, måltider, betalt,
herav trekkfritt, og for utlandet tabellens beløp, valuta, kurs og
kursdato. Raden er bevis, og triggeren holder den fast. Skjemaet viser
serverens tall (`POST …/reise/preview` kjører samme kode), aldri sine
egne.

**Bokføring og utbetaling.** Godkjenning posterer hele beløpet (7150
diettkostnad som standard) mot mellomregning; bilagslinjen bærer
reisemål, tidsrom og antall døgn. Dietten utbetales med lønnen, som
kjøregodtgjørelsen (se «Utbetaling»): den trekkpliktige delen går i
trekkgrunnlaget og avgiftsgrunnlaget, den trekkfrie bare til netto.

I a-meldingen (`IncomeKind::Travel`, sjekket mot Skatteetatens
inntektskombinasjoner R1 2026), alle som `utgiftsgodtgjoerelse`:

| Del | Beskrivelse | `antall` | Aga | Trekk |
| --- | --- | --- | --- | --- |
| trekkfri kost, hotell | `reiseKostMedOvernattingPaaHotell` | døgn | nei | nei |
| trekkfri kost, pensjonat/brakke/hybel uten kjøkken | `reiseKostMedOvernattingPaaHybelUtenKokEllerPensjonatEllerBrakke` | døgn | nei | nei |
| trekkfri kost, privat/hybel med kjøkken | `reiseKostMedOvernattingPaaHybelMedKokEllerPrivat` | døgn | nei | nei |
| trekkfri kost, dagreise | `reiseKostUtenOvernatting` | dager | nei | nei |
| trekkfritt nattillegg | `reiseNattillegg` | døgn | nei | nei |
| kost over trekkfri sats | `reiseKost` | ingen | ja | ja |
| nattillegg over trekkfritt beløp, helt | `reiseAnnet` | ingen | ja | ja |

Det påbegynte døgnet meldes under overnattingsstedet det følger.
`payroll_travel` (migrasjon 0087) er øyeblikksbildet per krav og
beskrivelse, så meldingen kan bygges fra kjøringen for alltid.

Ikke bygget: statens kompensasjonstillegg (§ 12, særskilt for
statsansatte), reduksjonen fra døgn 29, langtransportsjåfører, kost
etter regning («særlige grunner»), og reiser som skifter sats ved
nyttår.

## Firmakort

Et kjøp med firmakort er **firmaets penger**: kvitteringen er
dokumentasjon, ikke et krav om refusjon. Den går gjennom samme
utkast → innsendt → godkjent-flyt (innboks-disiplinen holder
kvitteringen fast), men godkjenningen krediterer **kortets konto** i
stedet for gjelden til den ansatte (2910), og kravet er oppgjort i samme
transaksjon (`utbetalt` med godkjenningsbilaget som oppgjør) — det er
ingenting å betale ut, og det havner verken i lønnskjøringen eller
betalingslisten.

**Kortregisteret** (`company_card`, migrasjon 0088; Bank → Firmakort,
`BANK_AVSTEM`): navn, de **fire siste sifrene** (aldri kortnummeret),
kortholder og kontoen kortet bokføres mot. Kontoen er permanent —
godkjente kjøp står på den — mens navn, kortholder og «i bruk» kan
endres; et kort slettes aldri. Ett aktivt kort per konto: et debetkort
kan stå på bankkontoen det trekker fra (1920), et kredittkort trenger en
egen konto (for eksempel en underkonto i 2390-serien). Et kort uten
kortholder er **delt**, og bare den som godkjenner utlegg kan føre kjøp
på det; ellers fører kortholderen sine egne.

**Kontoutskriften** for kortet importeres under Bank → Avstemming på
kortets konto, med samme camt.053/CSV-import som bankkontoene (kortets
konto vises der selv om den ligger i klasse 2). Det gir to ting:

- **Kortkjøp uten kvittering** (`GET …/expenses/card-lines`, Utlegg →
  «Legg ved kvitteringer»): linjer med penger ut på et korts konto som
  verken er avstemt mot hovedboken eller har en kvittering i arbeid.
  Kortholderen ser sine egne, den som godkjenner utlegg alle. På en
  bankkonto (19xx) deles kontoen med husleie, lønn og leverandører, så
  der regnes bare linjer som nevner kortets fire siste sifre som et
  eget tall (ikke inne i et lengre nummer) som kortkjøp; et kredittkorts
  egen konto har ikke annet. Et kort som tas ut av bruk beholder sine
  udokumenterte kjøp i listen, og kvitteringen kan fortsatt føres fra
  linjen — men ikke noe nytt på kortet.
- **Kvittering fra linjen** (`POST …/expenses/receipts?bank_transaction_id=`):
  bildet blir et utkast med linjens dato, beløp, tekst og kort, og
  tilhører kortholderen. En linje dokumenteres av én kvittering (unik
  indeks; et avvist eller forkastet krav frigjør den). Ved godkjenning
  avstemmes linjen mot kortkontoens bilagslinje **i samme transaksjon**
  — beløpene må være like, og er linjen allerede avstemt mot noe annet,
  nektes godkjenningen med beskjed.

En kvittering ført på kortet uten linjen (kontoutskriften har ikke
kommet ennå, eller den ble tatt med fra «Nytt utlegg») avstemmes på
vanlig måte: ved importen, eller — kom kontoutskriften først — rett
etter godkjenningen, med samme regel som importen (likt beløp, nærmeste
dato innenfor fem dager, uavgjort overlates til et menneske) — for dette
kjøpets bilagslinje alene, ingenting annet på kontoen.

Ikke bygget: flere kort på samme konto (én kontoutskrift kan ikke si
hvilket kort en linje er), kortkjøp i valuta der kvitteringens beløp
er i utenlandsk valuta (kravet føres i kroner, med kontoutskriftens
beløp), og påminnelse til kortholderen utenfor portalen.

## Agenten

En agent (docs/agent.md — en integrasjon med en rolle) kan registrere
og sende inn utleggskrav for de ansatte. **Den avgjør dem aldri.**

- **Fra innboksen.** Ansatte sender kvitteringer til selskapets
  e-post-inn-adresse, og de lander i bilagsinnboksen ved siden av
  leverandørfakturaene. Den som rydder innboksen — en bokfører eller en
  agent — gjør en kvittering betalt med egne penger til et utkast for
  den ansatte (`utlegg_fra_innboks`, docs/bilagsinnboks.md). Fra disk:
  `utlegg_registrer`.
- **Lese og fylle ut.** `utlegg_kvittering` viser modellen selve
  bildet; `utlegg_forslag` gir regnmeds egen lesing (tekstlag eller
  tekstgjenkjenning). Agenten fyller ut dato, beløp med mva og
  beskrivelse (`utlegg_oppdater`) og sender inn (`utlegg_send_inn`).
  Kravet er registrert av agenten på den ansattes vegne, og begge ser
  det.
- **Et menneske avgjør.** Godkjenning nektes i transaksjonen
  (`approve_expense`) når den som godkjenner er en maskin, uansett hva
  rollen gir; avvisning og utbetaling nektes med 403, og en
  betalingsliste med utleggskrav kan ikke godkjennes av en agent
  (`approve_run`). En agent med
  `bokforing`-rollen har `UTLEGG_GODKJENN` og blir likevel stoppet —
  regelen ligger på hvem som avgjør, ikke i rollen. Med attestering på
  er dessuten den som registrerte kravet utelukket som godkjenner, slik
  det alltid har vært.
- **Kravet er et menneskes.** Et utleggskrav kan ikke tilhøre en
  agent: uten `person_id` ville kravet blitt agentens eget, og det
  nektes (`create_draft`, `expense_from_inbox`).
- **Den ansattes egne utkast** forblir den ansattes: en agent ser bare
  utkastene den selv har registrert.

Rolleprompten `bokforer` har fått en fast arbeidsrekkefølge for dette
(UTLEGG), og prompten `ansatt` sier ikke lenger at den sender inn
utlegg: et utlegg tilhører personen som la ut pengene, og agenten er
ikke den personen.

Ikke bygget: en knapp i portalen for å gjøre et innboksdokument til et
utleggskrav (endepunktet finnes), og at agenten foreslår mva-kode eller
konto — det velger den som godkjenner.

## Utbetaling

Tre veier ut, valgt av hva kravet er — aldri av hva som er lettest:

**Kjøregodtgjørelse og diett til en ansatt utbetales med lønnen.** Delen over
den trekkfrie satsen er trekkpliktig og utløser arbeidsgiveravgift, og
bare en lønnskjøring kan trekke skatt. Begge delene skal i a-meldingen
for måneden de utbetales (Skatteetatens veiledning, «Trekkfri
bilgodtgjørelse» og «Trekkpliktig bilgodtgjørelse»). Lønnskjøringen
(`payroll::run_payroll`) tar derfor med den ansattes godkjente,
ubetalte kjøringer datert til og med utbetalingsdatoen:

- den trekkpliktige delen går inn i trekkgrunnlaget og
  avgiftsgrunnlaget, men gir ikke feriepenger;
- den trekkfrie delen og passasjertillegget går bare til netto;
- bilaget debiterer mellomregningen godkjenningen krediterte, så
  gjelden til den ansatte går i null;
- kravene settes `utbetalt` med lønnsbilaget, i samme transaksjon;
- `payroll_expense` (migrasjon 0086) lagrer hvordan hvert krav ble
  delt — et øyeblikksbilde, så a-meldingen kan bygges fra kjøringen
  for alltid. Et krav kan bare betales av én kjøring (unik nøkkel).

I a-meldingen (`regnmed-core::amelding::IncomeKind`, kodene sjekket mot
Skatteetatens inntektskombinasjoner R1 2026), alle som
`utgiftsgodtgjoerelse` med kilometerne som `antall`. Tallene er
regnearkets «gml.kode» — den gamle lønnsoppgavekoden, ikke et
kombinasjonsnummer, som arket ikke har:

| Del | Beskrivelse | Gammel kode | Aga | Trekk |
| --- | --- | --- | --- | --- |
| trekkfri, bil | `kilometergodtgjoerelseBil` | 711 | nei | nei |
| over trekkfri sats, bil | `kilometergodtgjoerelseBil` | 153 | ja | ja |
| passasjertillegg | `kilometergodtgjoerelsePassasjertillegg` | 714 | nei | nei |
| andre kjøretøy | `kilometergodtgjoerelseAndreFremkomstmidler` | 714 | nei | nei |

Passasjertillegget meldes med passasjerkilometer (km × passasjerer),
som er det tillegget betales per. Det er vår lesning — veiledningen
sier bare «antall kilometer». Andre kjøretøy har ingen trekkpliktig
kombinasjon; 2026-satsene gir heller ingen overskytende del, og skulle
de gjøre det, nekter a-meldingen med beskjed om å melde det særskilt.

Lønnsslippen viser den trekkpliktige delen som en lønnslinje og den
trekkfrie mellom trekket og «Til utbetaling». En kjøring til en ansatt
i et løpende arbeidsforhold kan ikke betales manuelt (`pay` nekter). En
person som ikke er ansatt — eller ikke lenger er det, så ingen kjøring
kan ta med vedkommende — har ikke noe arbeidsforhold regnmed kan melde;
godkjenningen sier det høyt, og kravet betales da manuelt og meldes
særskilt.

**Utlegg (kvitteringer) går i betalingslisten** (docs/betaling.md)
sammen med leverandørene, til kontonummeret i ansattregisteret — det
eneste stedet regnmed har en persons kontonummer. Et krav betales i sin
helhet. Mens det ligger i en liste som ikke er betalt, kan det verken
betales manuelt eller legges i en ny liste, og når listen registreres
betalt settes kravet `utbetalt` med utbetalingsbilaget.

**Manuelt:** `POST …/expenses/{id}/pay` — debet mellomregningskontoen
fra godkjenningen, kredit bank (1920 som standard), én transaksjon med
statusovergangen. Bankavstemmingen matcher utbetalingsbilaget mot
kontoutskriften som ethvert annet bilag.

## Viderefakturering

Et godkjent krav merket «viderefaktureres» (det krever prosjekt) tas med
på en faktura til kunden — samme skjema som ufakturerte timer, og
prosjektets kunde (docs/dimensjoner.md) er forslaget, aldri en regel.
`POST …/invoices` tar `expenses: [{expense_id, treatment}]`, og kravene
låses av fakturaen i samme transaksjon (`expense_invoice`, migrasjon
0086; primærnøkkelen gjør en ny viderefakturering umulig). Det krever
`UTLEGG_LES_ALLE` i tillegg til `FAKTURA_SKRIV`.

- **`resale` — videresalg.** Kostnaden er selskapets, og å fakturere
  den er et salg: linjen er kostnaden godkjenningen bokførte (etter
  fradrag for inngående mva), på 3000 med kode 3 (25 %) som timene. Et
  selskap som ikke er mva-registrert på fakturadatoen får kode 7 og
  ingen mva (mval. § 15-11; samme regel som andre avledede linjer,
  `settings`). Kjøregodtgjørelse kan bare viderefaktureres slik.
- **`disbursement` — utlegg.** Kjøp gjort i kundens navn og for kundens
  regning holdes utenfor beregningsgrunnlaget (mval. § 4-1 annet ledd
  bokstav a): ingen påslag, og det føres som utlegg, ikke som kostnad.
  regnmed tillater det derfor bare for et krav som ble godkjent UTEN
  fradrag for inngående mva og på en balansekonto (klasse 1, f.eks.
  1579). Fakturalinjen krediterer den kontoen uten mva-kode, så den
  går i null. Er kravet bokført som kostnad eller med mva-fradrag,
  nekter regnmed og ber om videresalg. Kunden skal ha originalbilaget;
  det sendes ikke med fakturaen automatisk ennå.

`GET …/expenses/billable` lister godkjente krav som venter, med kostnaden
et videresalg fakturerer og om kravet kan gå som utlegg.

Kjente begrensninger:

- **EHF:** en utleggslinje har ingen mva-kode, og EHF-renderen gjør en
  linje uten kode til kategori Z (nullsats). Riktig PEPPOL-kategori for
  et utlegg utenfor grunnlaget er ikke avklart, så regnmed gjetter
  ikke; PDF-en og posteringen er riktige.
- **Kreditnota frigjør ikke kravet.** Krediteres fakturaen, står kravet
  fortsatt som viderefakturert — som timer som er fakturert. En ny
  faktura må da lages med vanlige linjer.

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
- `POST /companies/{id}/inbox/{doc}/expense` — en kvittering i innboksen blir et utkast for `person_id`
- `GET  /companies/{id}/cards` (`?usable=true`), `POST /companies/{id}/cards`, `PUT /companies/{id}/cards/{card_id}` — kortregisteret
- `GET  /companies/{id}/expenses/card-lines` — kortkjøp uten kvittering
- `POST /companies/{id}/expenses/receipts?filename=&bank_transaction_id=` — kvittering til en kortlinje
- `GET  /companies/{id}/expenses/reise?dato=` — selskapets satsvalg, begge settene satser og statens landstabell
- `GET  /companies/{id}/expenses/reise/days?departure_at=&return_at=&overnight=` — antall døgn
- `POST /companies/{id}/expenses/reise/preview` — dietten reisen ville fått, med døgnlinjene

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

Bevisst utenfor så langt: KI-lesing av kvitteringen (agent-fasen), mva fordelt på flere satser
per kvittering, utlegg (kvitteringer) utbetalt med lønnen, og
originalbilaget sendt med fakturaen ved viderefakturering som utlegg.

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
- `crates/regnmed-db/tests/payroll.rs::mileage_is_paid_with_pay_and_reported_by_part`
  — to kjøringer (én med passasjer) i marslønnen: trekk av 35 % på
  lønn + overskytende del, aga på det samme og ikke på refusjonen (sett
  feile), 2910 i null, kravene `utbetalt`, en kjøring datert etter
  utbetalingsdatoen venter til april, manuell utbetaling nektet, og
  a-meldingens fire inntekter med kilometer; etter at arbeidsforholdet
  er avsluttet betales et nytt krav manuelt.
- `regnmed-core/src/amelding.rs::mileage_is_reported_by_part_with_its_kilometres`
  — kodene, flaggene og `antall` validert mot XSD-en; en refusjon i
  avgiftsgrunnlaget gir F185-avvik (sett feile).
- `regnmed-core/src/payroll.rs::the_taxable_allowance_is_withheld_and_the_rest_only_paid`
  — trekk på den trekkpliktige delen også uten lønn.
- `crates/regnmed-api/tests/grupper/payments.rs::receipt_claims_are_paid_from_the_payment_list`
  — uten ansattkonto nektet, delbeløp nektet, i liste = ikke manuelt og
  ikke i ny liste (sett feile), pain.001 til ansattkontoen, oppgjøret
  setter kravet utbetalt og 2910 i null.
- `crates/regnmed-api/tests/grupper/expenses.rs::billable_claims_are_rebilled_once`
  — videresalg av hotell netto etter mva-fradrag og kjøring, utlegg på
  1579 uten mva (1579 i null), utlegg nektet for et krav med
  mva-fradrag, prosjektets kunde som forslag, bare én gang (sett
  feile), og uten mva (kode 7) etter at selskapet er ute av
  mva-registeret (sett feile).

- `crates/regnmed-api/tests/grupper/agent.rs::an_agent_files_a_claim_and_a_human_decides_it`
  — en `bokforing`-agent finner den ansatte, gjør en kvittering i
  innboksen til et utkast (dokumentet får status `utlegg`, bare én
  gang), ser kvitteringen som bilde byte for byte, fyller ut og sender
  inn; et krav uten `person_id` nektes; en PDF vises ikke som bilde;
  agenten nektes å godkjenne (sett feile) og å avvise, et menneske
  godkjenner; vertsagenten gir bildet videre til leverandøren som
  `image_url` etter verktøysvarene, og bare det nyeste bildet følger
  med videre (sett feile).
- `crates/regnmed-api/tests/grupper/payments.rs::receipt_claims_are_paid_from_the_payment_list`
  — en agent kan ikke godkjenne en betalingsliste med utleggskrav (sett
  feile).
- `crates/regnmed-api/tests/grupper/expenses.rs::company_card_purchases_are_documented_not_reimbursed`
  — kortregisteret (fire sifre, ett kort per konto, bare `BANK_AVSTEM`),
  kortholder ser sitt kort og sine linjer, en kollega verken kortet,
  linjene eller retten til å føre på det; kontoutskrift importert på en
  kredittkortkonto i klasse 2; kvittering fra linjen med linjens dato
  og beløp, lenken beholdt når utkastet lagres, én kvittering per linje;
  godkjenning krediterer kortkontoen, 2910 urørt, kravet oppgjort,
  linjen avstemt i samme transaksjon (sett feile), en kvittering ført
  uten linjen avstemmes rett etter godkjenningen (sett feile); et
  debetkort på 1920 ser bare linjene som nevner sine fire sifre som eget
  tall (sett feile); et kort tatt ut av bruk beholder sin udokumenterte
  linje, som fortsatt kan få kvittering, mens nye kjøp på kortet nektes
  (sett feile).
- `regnmed-core/src/travel.rs` — døgntelling (6-timersgrensen, 28
  døgn), satsforskriftens egne tall (554 kr for hotell med frokost),
  statens overskudd, en natt i det påbegynte døgnet, kort reise med
  overnatting, nattillegg uten frokosttrekk, nattillegg over
  trekkfritt beløp trekkpliktig i sin helhet, timebånd, halv landssats
  ute, alle tre måltider gir null.
- `regnmed-core/src/amelding.rs::travel_is_reported_by_lodging_with_its_dogn`
  — beskrivelsene, `antall` på de trekkfrie og ikke på `reiseKost`,
  XSD-validert, bare overskuddet i avgiftsgrunnlaget.
- `crates/regnmed-db/tests/payroll.rs::diett_is_paid_with_pay_and_reported_by_lodging`
  — statens satser: hotellnatt med frokost + påbegynt døgn og en
  dagreise gjennom ekte utkast/innsending/godkjenning; trekk og aga på
  overskuddet, 2910 i null, a-meldingens linjer med døgn, manuell
  utbetaling nektet, betalt bare én gang.
- `crates/regnmed-api/tests/grupper/expenses.rs::a_trip_is_one_reiseregning`
  — satsene og landstabellen, døgntelling, en reise fram i tid nektet,
  Berlin etter skattefrie
  satser (som hjemme) og etter statens (EUR omregnet per døgn), kjøring
  og hotell under reisen (kjøringen inn, hotellet venter på
  kvittering), døgnlinjene lagret og låst i databasen, posteringen med
  reisemål og tidsrom.
- `ui/portal/src/sections/utlegg/expenseMath.test.js` — hvilke døgn som
  har en natt, også over sommertidsskiftet.
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
