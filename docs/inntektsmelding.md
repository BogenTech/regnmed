# Inntektsmelding til Nav — forslag

**Status: FORSLAG, ikke bygget** (2026-11-26, #184). Til det er bygget,
må selskapet sende inntektsmeldingen selv, på nav.no eller i et annet
system. docs/lonn.md sier det samme.

Når en ansatt er syk lenger enn arbeidsgiverperioden (16 dager), ber Nav
arbeidsgiveren om en **inntektsmelding**: hvilken arbeidsgiverperiode
som ble brukt, hva den ansatte tjener i måneden, og om arbeidsgiveren
krever refusjon. Nav bruker den til å beregne sykepengene. Kommer den
ikke, kan utbetalingen bli forsinket (nav.no, under). For foreldre- og
svangerskapspenger og for omsorgs-, pleie- og opplæringspenger finnes
tilsvarende inntektsmeldinger.

## 1. Kilden

Alt i dette avsnittet er lest 2026-11-26. Kildene er Navs egne:

- **Wikien** til [navikt/sykepenger-im-lps-api](https://github.com/navikt/sykepenger-im-lps-api/wiki),
  sist endret 2026-11-04. Den er Navs dokumentasjon for lønns- og
  personalsystemer (LPS) og har sidene om tilgang, forespørsel,
  inntektsmelding, inntekt, Dialogporten, testdata og vilkår.
- **OpenAPI-spesifikasjonen** for sykepenge-API-et i Navs testmiljø,
  `https://sykepenger-api.ekstern.dev.nav.no/swagger/documentation.yaml`
  (OpenAPI 3.1.1, «Sykepenger API» 1.0.0). Der den sier noe wikien ikke
  gjør, står det under med «spesifikasjonen».
- **Driftskonfigurasjonen** i Navs repoer (`.nais/prod.yaml`,
  `.deploy/prod-gcp*.json`, `application-prod-gcp.properties`) for
  vertsnavn, scope og Altinn-ressurs, og **kildekoden** der wikien ikke
  sier nok (`InnsendingUtils.kt`, `ForespoerselRepository.kt`). Det er
  Navs egen kode, men ikke dokumentasjon. Det som står under med
  «koden», må bekreftes i testmiljøet.
- READMEene til [navikt/fp-inntektsmelding-api](https://github.com/navikt/fp-inntektsmelding-api)
  og [navikt/k9-inntektsmelding-api](https://github.com/navikt/k9-inntektsmelding-api).
- [Sende inntektsmelding på nav.no](https://www.nav.no/arbeidsgiver/sende-inntektsmelding)
  (oppdatert 11. juni 2026) og [Inntektsmelding](https://www.nav.no/arbeidsgiver/inntektsmelding)
  (oppdatert 2. november 2026).
- Altinns metadata for tilgangspakker,
  `https://platform.altinn.no/accessmanagement/api/v1/meta/info/accesspackages/…`.

### Tre API-er, ett mønster

| Ytelse | Scope | Altinn-ressurs | Produksjon |
| --- | --- | --- | --- |
| Sykepenger | `nav:helseytelser/sykepenger` | `nav_sykepenger_inntektsmelding` | `https://sykepenger-api.nav.no` |
| Foreldre- og svangerskapspenger | `nav:inntektsmelding/foreldrepenger` | `nav_foreldrepenger_inntektsmelding` | `https://foreldrepenger-inntektsmelding-api.nav.no` |
| Omsorgs-, pleie- og opplæringspenger | `nav:inntektsmelding/sif` | `nav_sykdom-i-familien_inntektsmelding` | `https://k9-inntektsmelding-api.nav.no` |

Alle tre tar et Maskinporten-token for **systembruker** og sjekker i
Altinn at systemet har ressursen for organisasjonen i kallet. Alle tre
har forespørsel, innsending og henting av innsendte meldinger. k9-API-et
har i tillegg refusjonskrav for omsorgspenger, som ikke krever en
forespørsel.

Bare sykepenge-API-et er dokumentert for leverandører. Wikiens FAQ sier
at «kun sykepenger» har besluttet å ha API, og nav.no (juni 2026) sier at
inntektsmelding for de andre ytelsene «kan kun sendes inn manuelt». De
to andre repoene har likevel produksjonsadresser og ble endret denne
uken. Om de er åpne for leverandører, er ikke avklart (§5).

Resten av avsnittet gjelder sykepenger.

### Tilgang

- **Scope.** `nav:helseytelser/sykepenger` er åpent for alle
  (`accessibleForAll` i Navs konfigurasjon). Det legges på
  Maskinporten-klienten i Samarbeidsportalen («Tilgjengelig for alle»
  må hukes av for å finne det) og trenger ingen søknad.
- **Bare det ene scopet.** API-et godtar bare tokens med nøyaktig
  `nav:helseytelser/sykepenger`. Et token med flere scope avvises, og
  tokenet skal ikke veksles til et Altinn-token (wikien, «Autentisering
  og autorisasjon»).
- **Systembruker.** Systemet i Altinns systemregister må ha
  `access` til `nav_sykepenger_inntektsmelding`. Den ene ressursen
  dekker å hente forespørsler, sende inn og hente innsendte
  inntektsmeldinger og `GET /v1/inntekt`. Sykmelding, søknad og
  Dialogporten er egne ressurser (`nav_sykepenger_sykmelding`,
  `nav_sykepenger_soeknad`, `nav_sykepenger_dialog`), som regnmed ikke
  trenger (§4).
- **Selskap som rapporterer selv.** Ressursen
  `nav_sykepenger_inntektsmelding` legges i systemets `rights`, ved
  siden av Skatteetatens. Tilgangspakken «Lønn med personopplysninger av
  særlig kategori» (`lonn-personopplysninger-saerlig-kategori`), som
  wikien foreslår, er ikke et alternativ for regnmed: systemregisteret
  tar bare REGN-, REVI- og forretningsførerpakker (regel 2 i
  docs/gov.md), og pakken ville også gitt sykmelding og søknad, som
  regnmed ikke skal ha (§4).
- **Regnskapsfører.** Alle tre REGN-pakkene systemet er registrert med
  (`scripts/altinn-system.py`) inneholder
  `nav_sykepenger_inntektsmelding`, `nav_foreldrepenger_inntektsmelding`
  og `nav_sykdom-i-familien_inntektsmelding` (Altinns metadata for
  produksjon; TT02 er ikke sjekket). Klient-systembrukere er ennå ikke
  bygget (docs/gov.md), og om Navs API godtar et token for en
  klient-systembruker, sier ingen av Navs sider. Det må prøves.
- **Vilkår.** Bruken reguleres av Navs «Bruksvilkår – Nav sitt API for
  sykepenger – V1» (PDF lenket fra wikiens side «Vilkår for bruk av
  API-et»). Nav anser ikke vilkårene som oppfylt før leverandørens
  kontaktinformasjon er sendt til nav.prosjekt.2.inntektsmelding@nav.no.
- **Testmiljø.** `https://sykepenger-api.ekstern.dev.nav.no`, mot
  Altinn TT02 og test-Maskinporten. Testpersoner med sykmelding lages i
  Navs testdataverktøy Dolly. Tilgang til Dolly bestilles ved å sende
  leverandørens organisasjonsnummer til dolly@nav.no, og deretter gi
  hver person tilgang til tjenesten «Dolly» i Altinn (wikien,
  «Testdata»).

### Forespørselen

Når en sykmeldt søker sykepenger og fraværet går utover
arbeidsgiverperioden, lager Nav en **forespørsel** til arbeidsgiveren.
Den har en `navReferanseId`, og **inntektsmeldingen kan bare sendes
som svar på en forespørsel**:

> «Det er ikke mulig å sende inntektsmelding via APIet uten å ha først
> mottatt en forespørsel.» (wikien, «Forespørsel om Inntektsmelding»)

Feltene er `navReferanseId`, `loepenr`, `orgnr` (underenheten), `fnr`,
`status`, `sykmeldingsperioder`, `egenmeldingsperioder`,
`inntektsdato`, `arbeidsgiverperiodePaakrevd`, `inntektPaakrevd` og
`opprettetTid` (spesifikasjonen). Ingen diagnose.

Status er `AKTIV` (Nav venter), `BESVART` (Nav har godkjent en
inntektsmelding for den; kan fortsatt korrigeres) eller `FORKASTET`
(trukket tilbake, kan ikke besvares; som oftest kommer en ny `AKTIV`).

Nye forespørsler finnes på to måter: `POST /v1/forespoersler`, eller et
abonnement på Altinn-hendelser fra Dialogporten (webhook, scope
`altinn:events.subscribe` og `subscribe` på `nav_sykepenger_dialog`).
Om spørringen:

- `orgnr` er **underenheten**, og den må stemme nøyaktig (koden).
  Selskapets eget organisasjonsnummer gir ingen treff.
- `fraLoepenr` gir forespørsler med høyere `loepenr`. Men `loepenr` er
  radnummeret, og når status endres, skrives den på samme rad (koden).
  Et vannmerke på `loepenr` ser altså bare **nye** forespørsler, aldri at
  en blir `BESVART` eller `FORKASTET`.
- Høyst 1 000 per svar, med `X-Warning-limit-reached`
  (spesifikasjonen).

### Uten forespørsel

API-et tar ikke imot inntektsmeldinger Nav ikke har bedt om. Nav sier
grunnen er å slippe meldinger de ikke trenger (wikiens FAQ). I disse
tilfellene kommer ingen forespørsel, og meldingen sendes på nav.no:

- den ansatte har ikke meldt egenmeldingsdager, så Nav vet ikke at
  fraværet går utover arbeidsgiverperioden;
- arbeidsgiveren skal ikke betale hele eller deler av
  arbeidsgiverperioden, og fraværet er under 16 dager;
- søknaden er ikke elektronisk (ofte utenlandske sykmeldinger);
- sykmelding for enkeltstående behandlingsdager.

Flere arbeidsforhold i samme underenhet sendes som én samlet melding
gjennom API-et, med ett unntak som går på nav.no: gradert fravær fra
bare noen av arbeidsforholdene, med ulik lønn (wikien, «Flere
arbeidsforhold i samme underenhet»). regnmed har én ansatt per
fødselsnummer per selskap (migrasjon 0035) og kan ikke uttrykke flere
arbeidsforhold i dag.

En inntektsmelding sendt på nav.no kan **hentes** gjennom API-et:
`typeInnsending` skiller `FORESPURT_EKSTERN` (fra et system) fra
`ARBEIDSGIVER_INITIERT` (uten forespørsel) og meldinger sendt i Navs
eget skjema (koden; wikien, «Testdata»). regnmed kan altså se det som er
sendt utenfor regnmed.

### Inntektsmeldingen

`POST /v1/inntektsmelding` (spesifikasjonen). Påkrevd:
`navReferanseId`, `sykmeldtFnr`, `aarsakInnsending` (`Ny`/`Endring`),
`arbeidsgiverTlf`, `avsender` (systemnavn og -versjon), `agp`, `inntekt`,
`refusjon` og `naturalytelser`. `kontaktinformasjon` (navn på
kontaktperson) er valgfri. `agp`, `inntekt` og `refusjon` må være med,
men kan være `null`, og forespørselens to flagg styrer hvilke. Nav
avviser meldingen med en gang (`400`, koden) når:

- `arbeidsgiverperiodePaakrevd` er sann og `agp` mangler;
- `inntektPaakrevd` er sann og `inntekt` mangler, **eller** den er usann
  og `inntekt` er med;
- `aarsakInnsending` er `Endring` mens forespørselen er `AKTIV`, eller
  `Ny` når den er `BESVART`;
- forespørselen er `FORKASTET`, eller `sykmeldtFnr` ikke er forespørselens.

En `agp` Nav ikke ba om, tas imot, men bare hvis den viser minst én
arbeidsdag i starten av sykmeldingen (wikien, «Validering av
ikke-forespurt AGP»). `refusjon: null` betyr at arbeidsgiveren ikke
krever refusjon.

- **`agp`**: periodene i arbeidsgiverperioden, og `redusertLoennIAgp`
  (beløp og en av 15 begrunnelser) bare når arbeidsgiveren ikke betalte
  full lønn.
- **`inntekt`**: `beloep` er **månedsinntekten etter folketrygdloven
  § 8-28**, `inntektsdato` og `endringAarsaker`. Hovedregelen er
  gjennomsnittet av det som er rapportert i a-ordningen de tre siste
  kalendermånedene før `inntektsdato`, delt på tre også om en måned
  mangler. Fravikes den, oppgis årsaken: `VarigLoennsendring`, `Ferie`,
  `Permisjon`, `Nyansatt`, `NyStillingsprosent` og sju til, med datoer.
  Wikien har en tabell over hvilke inntektstyper som teller (fastlønn
  og timelønn ja; feriepenger, overtid og naturalytelser nei).
- **`refusjon`**: `beloepPerMaaned` når arbeidsgiveren betaler lønn
  under sykefraværet og krever pengene tilbake, og `endringer` (beløp
  0 fra en dato stopper refusjonen).
- **`naturalytelser`**: naturalytelser som faller bort under fraværet.

Beløpene er `number` i spesifikasjonen, altså kroner med desimaler.

Nav tar imot meldingen med `201` og en `innsendingId` (`409` ved
duplikat). Status er så `MOTTATT`, og etter noen minutter `GODKJENT`
eller `FEILET` med `valideringsfeil`. Den eneste regelen som kjøres
etterpå, er at inntekten må stemme med a-ordningen, eller ha en årsak.
Ellers `INNTEKT_AVVIKER_FRA_A_ORDNINGEN`. `GET /v1/inntekt?navReferanseId=…&inntektsdato=…`
gir de tre månedene og gjennomsnittet Nav selv validerer mot, og
wikien anbefaler å vise dem før innsending.

En **korrigering** sendes som en ny, komplett melding på samme
`navReferanseId` med `aarsakInnsending: Endring`. Den siste
`GODKJENT`-meldingen gjelder. En melding som `FEILET`, lar forespørselen
stå `AKTIV`, så den nye meldingen er `Ny`, ikke `Endring`.

### Regelen fra nav.no

- Inntektsmeldingen sendes «som hovedregel kun» etter at Nav har bedt om
  den, og for sykepenger så snart arbeidsgiverperioden er over.
- A-melding må være levert for alle de tre månedene før første
  fraværsdag.
- Refusjon: arbeidsgiveren oppgir alltid **full** lønn det kreves
  refusjon for, også ved gradert fravær.

## 2. Hva regnmed har, og hva som mangler

| Inntektsmeldingen trenger | I regnmed i dag |
| --- | --- |
| Underenhet (`orgnr` i forespørselen) | `employee.business_unit_orgnumber` (migrasjon 0073). Kolonnen kan være tom, og en ansatt uten underenhet blir aldri spurt etter |
| Den sykmeldte | `employee.fodselsnummer`, matchet mot `fnr` i forespørselen |
| Arbeidsgiverperioden | **Mangler.** regnmed registrerer ikke fravær i det hele tatt: ingen sykefravær, egenmelding eller permisjon. Forespørselen bærer sykmeldings- og egenmeldingsperiodene, og arbeidsgiverperioden må bekreftes av et menneske |
| Redusert lønn i arbeidsgiverperioden | **Mangler.** Lønnskjøringen betaler fast månedslønn eller førte timer og vet ikke om noen ble trukket for fravær |
| Månedsinntekt (§ 8-28) | **Delvis.** `payroll_line` har det regnmed selv har utbetalt, per utbetalingsmåned som i a-meldingen. Feriepenger skal ikke med, så regnmeds tall må skille lønn fra feriepenger, ikke summere alt. Måneder fra før selskapet tok i bruk regnmed finnes ikke. `GET /v1/inntekt` gir Navs eget tall |
| Årsak til å fravike hovedregelen | **Delvis.** `employee.last_salary_change` (0073) kan foreslå `VarigLoennsendring`, og `ansatt_fra` (0035) `Nyansatt`. Stillingsprosenten finnes (0073), men bare den gjeldende, så en endring har ingen dato. Ferie og permisjon er ikke registrert |
| Refusjon | **Mangler.** Om selskapet betaler lønn under sykefraværet er en avgjørelse per ansatt. Hverken kravet eller Navs utbetaling bokføres i dag |
| Naturalytelser | Lønnen har ingen naturalytelser (docs/lonn.md), så listen er alltid tom. Det er riktig for regnmeds lønn |
| `arbeidsgiverTlf` (påkrevd) | **Mangler.** Verken selskapet eller personene har telefonnummer |
| `kontaktinformasjon` | Navnet på personen som sender, speilet fra regnid |
| Tilgang | Ingen rett for fravær. `LONN_LES` er nærmest, men revisor har den |
| Token | `regnmed-api` bygger én `TokenProvider` per skinne med skinnens egne scope (`amelding::SCOPES`, `tax_card::SCOPES`, i `main.rs`). Sykepenger blir én til, med `nav:helseytelser/sykepenger` alene |

## 3. Flyten i regnmed

1. **Hent forespørsler.** En jobb i arbeideren kaller
   `POST /v1/forespoersler` daglig for hvert selskap med systembruker,
   én gang per **underenhet** de ansatte står på, uten vannmerke:
   hele listen hentes, som for trekkpålegg. Et selskap har en håndfull
   forespørsler, og bare hele listen viser at en er blitt `BESVART` eller
   `FORKASTET` (§1). En forespørsel lagres ordrett og innsettings-bart,
   og en statusendring blir en ny rad. Den knyttes til den ansatte på
   `fnr` og underenhet. En forespørsel uten ansatt vises som et avvik og
   forsvinner ikke, og en ansatt uten underenhet vises som en mangel,
   siden ingen forespørsel for den ansatte kan finnes.
2. **Varsle** dem som har den nye retten (§4), med frist «så snart
   arbeidsgiverperioden er over».
3. **Forslag.** regnmed fyller ut et utkast og følger forespørselens
   flagg (§1): arbeidsgiverperioden fra egenmeldings- og
   sykmeldingsperiodene bare når den er forespurt, og inntekten bare når
   den er forespurt — fra `GET /v1/inntekt`, med regnmeds egne tall ved
   siden av. `VarigLoennsendring` eller `Nyansatt` foreslås når
   `last_salary_change` eller `ansatt_fra` faller i beregningsperioden.
   Et avvik mellom Navs og regnmeds tall vises, og ingen av dem velges
   stille.
4. **Et menneske sender.** Inntektsmeldingen er en erklæring fra
   arbeidsgiveren og sendes aldri automatisk. Agenter kan lage utkast,
   men ikke sende, på samme måte som forslagsretten i docs/agent.md.
5. **Lagre kvitteringen.** Det som ble sendt, og svaret, lagres
   innsettings-bart. Jobben henter status til meldingen er `GODKJENT`
   eller `FEILET`, og `FEILET` vises med Navs `valideringsfeil`. En
   korrigering er en ny innsending, aldri en retting, med
   `aarsakInnsending` etter forespørselens status (`Ny` etter `FEILET`,
   §1).
6. **Ingen forespørsel.** Portalen sier at meldingen må sendes på nav.no
   og lister tilfellene fra §1. Jobben i steg 1 henter også
   inntektsmeldinger sendt utenfor regnmed (`POST /v1/inntektsmeldinger`),
   så det som er sendt på nav.no, står ved siden av det regnmed sendte.

## 4. Avgjørelser

Disse er brukerens:

- **Bare sykepenger i første omgang?** Forslaget er ja: det er det eneste
  API-et Nav dokumenterer for leverandører. Foreldre- og pleiepenger
  følger samme mønster, med egne scope, og tas når Nav bekrefter at de er
  åpne (§5).
- **Polling eller hendelser.** Forslaget er daglig polling av hele
  listen (§3, steg 1). Hendelser krever et offentlig webhook-endepunkt,
  et ekstra scope og ressursen `nav_sykepenger_dialog`. Sykepenger
  venter ikke på minuttet.
- **Hente sykmelding og søknad?** API-et tilbyr det, men regnmed trenger
  dem ikke til inntektsmeldingen. Forslaget er nei, fordi det er
  helseopplysninger regnmed da ville lagret uten grunn.
- **To nye rettigheter**, fordi delegerbarhet er per rett
  (`Right::can_be_delegated`): én for å se forespørsler og lage utkast,
  som kan delegeres til en agent, og én for å sende, som ikke kan det —
  samme grunn som `MVA_SEND`: en innsending skal ha en person bak seg.
  Begge legges i `bokforing`-bunten (og dermed `admin`), men **ikke**
  hos `revisor`. Revisor har `LONN_LES`, men trenger ikke vite hvem som
  er syk.
- **Telefonnummer.** Enten et telefonnummer på selskapet (firmaopplysninger),
  eller på personen som sender. Forslaget er selskapet: Nav spør etter
  «telefonnummer arbeidsgiver».
- **Refusjon** er med i inntektsmeldingen fra start, som et felt
  brukeren fyller ut. Bokføringen av refusjonskravet og Navs utbetaling
  er en egen sak: Nav betaler med en oppgjørsrapport (Altinn-ressursen
  `nav_utbetaling_oppgjorsrapport-refusjon-arbeidsgiver`, i de samme
  pakkene), og den må avstemmes mot banken.
- **Oppbevaring.** Forespørsler og inntektsmeldinger er særlige
  kategorier (docs/kundevilkar.md, bilag 1). Når refusjonen bokføres,
  dokumenterer inntektsmeldingen et krav, og kan da være
  regnskapsmateriale. Hvor lenge de skal lagres, må bestemmes sammen
  med det, før det bygges.

## 5. Før noe kan prøves i Navs testmiljø

1. Maskinporten-testklienten har fått `nav:helseytelser/sykepenger`
   (selvbetjent i Samarbeidsportalen, ingen søknad).
2. regnmeds system i Altinns systemregister (TT02) har fått ressursen
   `nav_sykepenger_inntektsmelding` (`scripts/altinn-system.py update`),
   og testselskapets systembruker har godkjent endringen
   (`altinn-system.py change`, godkjennes med TestID).
3. Navs bruksvilkår er akseptert, og kontaktinformasjon er sendt til
   nav.prosjekt.2.inntektsmelding@nav.no.
4. BogenTech har tilgang til Dolly (organisasjonsnummer til
   dolly@nav.no, deretter tjenesten «Dolly» delegert i Altinn), og en
   forespørsel er laget slik wikiens side «Testdata» beskriver:
   - daglig leder for testselskapet (311032240) er importert i Dolly fra
     Tenor;
   - en testperson har arbeidsforhold i Aa-registeret og inntekt i
     Inntektstub på en **underenhet** av testselskapet (begge påkrevd);
   - sykmeldingen er tilbakedatert, sendt til arbeidsgiveren, og
     søknaden er sendt i «Ditt sykefravær» med TestID, med sykmelding og
     egenmelding over 16 dager til sammen;
   - samme person er ansatt i regnmed med samme fødselsnummer og
     underenhet.
5. Avklart med Nav: om foreldrepenger- og k9-API-ene er åpne for
   leverandører.

Inntekten Nav validerer mot i testmiljøet, er den som er lagt inn i
Dolly, ikke a-meldingene regnmed har levert til Skatteetatens
testmiljø. En `GODKJENT` i test viser derfor at veien gjennom API-et
virker, ikke at regnmeds eget tall etter § 8-28 er riktig. Det siste
testes mot regnmeds egne lønnslinjer.

## 6. Ferdig når

En forespørsel fra Navs testmiljø blir hentet, besvart med en
inntektsmelding fra regnmed, og får status `GODKJENT`, og en korrigering
av den samme meldingen blir også `GODKJENT`. Samme krav som for
a-meldingen og trekkpålegg.
