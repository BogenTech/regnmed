# Trekkpålegg — forslag

**Status: FORSLAG, ikke bygget** (2026-11-25, #334). Til det er bygget,
trekker lønnskjøringen ingen trekkpålegg, og både docs/lonn.md og
forhåndsvisningen av kjøringen sier det.

Et trekkpålegg (utleggstrekk i lønn) pålegger arbeidsgiveren å holde
tilbake en del av den ansattes lønn og betale den til innkrevings-
myndigheten. Arbeidsgiveren er trekkpliktig; et pålegg som ikke
gjennomføres, er arbeidsgiverens ansvar. Skatteetaten oppgir
innkrevingsloven § 28 og tvangsfullbyrdelsesloven § 1-10 som grunnlag
for tjenesten (tjenestebeskrivelsen, under).

## 1. Kilden

Alt i dette avsnittet er lest 2026-11-25 i Skatteetatens egen
API-dokumentasjon, `docs/api/trekkpaalegg.md` i
[Skatteetaten/api-dokumentasjon](https://github.com/Skatteetaten/api-dokumentasjon)
(publisert på skatteetaten.github.io/api-dokumentasjon/api/trekkpaalegg),
og i [tjenestebeskrivelsen for sluttbrukersystemer](https://www.skatteetaten.no/samarbeidspartnere/sluttbrukersystemer/trekkpalegg-sbs/).
OpenAPI-spesifikasjonen ligger på SwaggerHub (`skatteetaten/trekkpaalegg-app`,
versjon 1.4) og er **ikke lest ennå** — feltnavnene under er fra
dokumentasjonens eksempler og feltforklaringer.

### Tilgang

- Scope `skatteetaten:trekkpaalegg`, med **systembruker** når vi henter
  for en kunde. Systembrukerforespørselen må da også ha ressursen
  `ske-informasjon-om-trekkpaalegg` (tilgangsressursen «Informasjon om
  trekkpålegg»).
- Test-scopet bestilles med e-post til fremtidensinnkreving@skatteetaten.no
  med virksomhetens ekte organisasjonsnummer. Samme adresse lager
  testdata (trekkpålegg på syntetiske personer i et syntetisk selskap)
  når integrasjonen når tjenesten.
- Produksjon: Skatteetaten ber om en oppsummering av testen, og
  produksjonstilgang gis ikke før den er levert. Avtalen («vilkår for
  bruk») signeres da.
- Tjenestebeskrivelsen: leverandøren må være integrert mot minst ett
  innrapporterings-API (a-meldingen holder), og kunden må ha
  innrapportert gjennom systemet minst én gang. Data brukes bare til å
  gjennomføre trekkpålegg for kunden; vi er databehandler.

### Endepunkter

| Metode | Sti | Hva |
| --- | --- | --- |
| GET | `/api/trekkpaalegg/v1` | Siste versjon av alle gjeldende trekkpålegg for den trekkpliktige. `fraSekvensnummer` + `maksAntall` gir bare det som er endret siden et vannmerke. |
| GET | `/api/trekkpaalegg/v1/{trekkid}/{trekkversjon}` | Én bestemt versjon. |

Test: `https://api-test.sits.no`. Produksjon:
`https://api.skatteetaten.no/api/trekkpaalegg/v1`. Svaret er JSON.
Skatteetaten sier at SBS må **polle**; en hendelse via Dialogporten skal
komme senere. Feilkodene er `KB-001`–`KB-009`.

### Et trekkpålegg

```json
{
  "trekkid": "10006",
  "skyldner": "10108412345",
  "opprettet": "2025-10-15T15:00:00.00Z",
  "saksnummer": "TREKK/2025/6",
  "trekkstatus": "aktiv",
  "trekkpliktig": "123456789",
  "trekkversjon": 2,
  "sekvensnummer": 290,
  "betalingsinformasjon": {
    "kidnummer": "6487719756",
    "kontonummer": "70213997155",
    "betalingsmottaker": "971648199"
  },
  "trekkstoerrelseForPeriode": [
    { "startdato": "2025-10-01", "sluttdato": "2025-10-15",
      "trekkbeloep": { "trekkbeloep": 8000 } },
    { "startdato": "2025-10-16",
      "trekkbeloep": { "trekkbeloep": 6000 } }
  ]
}
```

Reglene Skatteetaten selv skriver:

- **Hvilken periode gjelder**: den som dekker **lønnsutbetalingsdatoen**.
- **`trekkprosent`** trekkes av **brutto inntekt**, ved hver utbetaling.
- **`trekkbeloep`** er et beløp per måned. Er det flere utbetalinger i
  måneden, er det arbeidsgiverens ansvar at hele beløpet trekkes i løpet
  av måneden, ved minst én av dem.
- Begge kan være 0.
- **`trekkstatus: avsluttet`**: det skal ikke lenger trekkes.
- `trekkid` er stabil, `trekkversjon` øker med 1, og `sekvensnummer` er
  et globalt, alltid stigende løpenummer som kan brukes som vannmerke.
- `kidnummer` er fast for en `trekkid`. `kontonummer` og
  `betalingsmottaker` er innkrevingsmyndighetens, felles for alle.
- `skyldner` er den ansattes fødsels- eller D-nummer.

### Reglene for arbeidsgiveren

Lest 2026-11-25 i Skatteetatens veiledning [Om trekk i lønn hos
ansatt](https://www.skatteetaten.no/bedrift-og-organisasjon/betaling-og-innkreving/tema/trekk-i-lonn-hos-ansatte/om/),
[Betale trekk i lønn](https://www.skatteetaten.no/bedrift-og-organisasjon/betaling-og-innkreving/tema/trekk-i-lonn-hos-ansatte/betale/),
[Utleggstrekk i a-meldingen](https://www.skatteetaten.no/bedrift-og-organisasjon/arbeidsgiver/a-meldingen/veiledning/utleggstrekk)
(oppdatert 25.02.2026) og skattebetalingshåndboken 14-4.4 og 14-5.7.
Håndboken sier selv at den ikke er oppdatert for innkrevingsloven fra
1.1.2026; der den og veiledningen spriker, gjelder veiledningen.

- **Samordnet trekk.** Fra 2026 har en ansatt **ett** samordnet
  utleggstrekk. Når et samordnet trekk settes i gang, skal alle andre
  trekk avsluttes. Påleggene fra API-et er samordnede. Trekk etter den
  gamle ordningen kommer som brev og løper til arbeidsgiveren får
  beskjed om å stoppe dem; de kommer aldri gjennom API-et og
  registreres for hånd (§2, steg 1).
- **Grunnlaget for prosenten** er bruttolønnen, regnet på samme grunnlag
  som forskuddstrekket. Naturalytelser og utgiftsgodtgjørelser holdes
  utenfor. Trekkfri kjøregodtgjørelse og diett er derfor ikke med; den
  trekkpliktige delen av en bilgodtgjørelse er med. Feriepenger er med,
  og sommer- og desemberreglene for forskuddstrekket gjelder ikke for
  utleggstrekk — i måneder med feriepenger eller halv skatt trekkes det
  pålegget sier.
- **Ikke nok å trekke i.** Arbeidsgiveren skal trekke så mye som mulig,
  aldri mer enn pålegget. Er årsaken ikke synlig i a-meldingen (sykdom
  og permisjon er det), skal arbeidsgiveren kontakte
  innkrevingsmyndigheten.
- **Frist.** Trekket betales til innkrevingsmyndigheten senest første
  virkedag etter lønnsutbetalingen, med påleggets KID (én KID per
  ansatt).
- **Fratrådt.** For samordnet trekk er det nok at sluttdatoen meldes i
  a-meldingen.
- **Arbeidsgiveren er ikke part.** Størrelsen på trekket — også
  hensynet til hva den ansatte trenger til livsopphold — er
  innkrevingsmyndighetens avgjørelse. Arbeidsgiveren trekker det
  pålegget sier.

### A-meldingen

A-meldingen v2.3 (vendored i docs/amelding/) har `utleggstrekk` per
inntektsmottaker — `beskrivelse`, `beloep` (hele kroner),
`datoForUtleggstrekk` (valgfri) — og `sumUtleggstrekk` i
betalingsinformasjonen. Veiledningen sier:

- Trekket rapporteres med beskrivelsen **`utleggstrekkSamordnet`**
  (`utleggstrekkSkatt` er den gamle ordningens). Skjemaet har bare
  `beskrivelse` som tekstfelt for dette (`Utleggstrekkbeskrivelse` er en
  ubegrenset `xsd:string`), så det er der koden står — en slutning fra
  skjemaet og veiledningen sammen, som testinnsendingen må bekrefte.
- **Beløpet har minustegn.**
- Datoen er lønnsutbetalingsdatoen.
- «Sum utleggstrekk å betale» er summen av samordnede trekk og
  skattetrekk i samme a-melding.

## 2. Flyten i regnmed

1. **Hent.** En jobb i arbeideren henter **daglig**
   `GET /api/trekkpaalegg/v1` for
   hvert selskap som har systembruker og har levert minst én a-melding
   gjennom regnmed, med `fraSekvensnummer` fra forrige vannmerke. Jobben
   henter side for side til en side kommer tilbake med færre enn
   `maksAntall` — én side kan ikke antas å være alt. Vannmerket flyttes
   til høyeste `sekvensnummer` i **samme transaksjon** som versjonene
   lagres, så et kall som feiler halvveis hentes på nytt neste gang i
   stedet for å hoppes over. Om `fraSekvensnummer` er inkluderende, er
   ikke dokumentert (§4), så en versjon kan komme igjen: lagringen er
   idempotent — en versjon som alt finnes, hoppes over. «Finnes» avgjøres
   på de tolkede feltene, ikke på bytene, så samme pålegg med feltene i
   en annen rekkefølge er det samme. Er feltene faktisk ulike for samme
   (`trekkid`, `trekkversjon`), lagres den nye kopien som en
   **konflikt** ved siden av originalen (heller ikke den kan endres),
   pålegget vises som et avvik i portalen og forhåndsvisningen, og
   hentingen går videre. Å stoppe ville ikke hjulpet: originalen kan
   ikke rettes, så hver henting ville stoppet på samme versjon og ingen
   nye pålegg kommet inn. Kjøringen trekker etter originalen til
   selskapet har kontaktet innkrevingsmyndigheten og en ny versjon
   kommer.

   Endringslisten alene kan ikke vise at et pålegg er **borte**: at et
   stoppet pålegg kommer tilbake som `avsluttet` og ikke bare faller ut
   av listen over «gjeldende» pålegg, er ikke bekreftet (§4). Faller det
   ut, ville regnmed trekke etter siste lagrede versjon hver måned uten
   grunnlag. Derfor henter den daglige jobben også hele listen, uten
   `fraSekvensnummer`, og lagrer når hvert pålegg sist ble sett i den
   (logget før kallet, som alle kall). Et **hentet** pålegg som er
   aktivt i regnmed, men manglet i siste vellykkede helliste, trekkes
   **ikke** — men bare hvis regnmed alt hadde det da listen ble hentet:
   et pålegg lagret etter hellisten kan ikke ha vært i den, og regnes
   ikke som borte; det vises som et avvik, og advarselen ber selskapet kontakte
   innkrevingsmyndigheten. Manuelt registrerte pålegg (under) finnes
   aldri i listen og omfattes ikke. Forhåndsvisningen og bokføringen
   leser begge det lagrede, aldri Skatteetaten direkte: forhåndsvisningen
   lagrer ingenting (docs/lonn.md), og de to må regne ut samme sett
   (punkt 2 under Godkjenning i §3). Feiler hellisten, gjelder
   tredagersadvarselen (punkt 3 samme sted).
   Samme henting — endringene og hellisten — kan startes fra portalen
   («Hent trekkpålegg»). Som for
   skattekortet skrives forespørselen **før** kallet, og en feil lagres
   med koden.

   Kravet om en levert a-melding er API-ets, ikke vårt: et selskap som
   kommer til regnmed med et løpende pålegg, får det ikke før etter
   første a-melding herfra. Har selskapet aldri hentet med hell, sier
   derfor forhåndsvisningen av kjøringen at eventuelle pålegg selskapet
   alt har, ikke er med, og at de må **registreres for hånd** (under).
   Å trekke dem utenfor regnmed går ikke: a-meldingen for måneden lages
   her, og den ville da verken vist trekket eller stemt med det som ble
   utbetalt.

   **Manuelt registrert pålegg.** Den som har `LONN_SKRIV`, kan legge
   inn et pålegg fra brevet: ansatt, KID, beløp eller prosent, perioden,
   a-meldingens beskrivelse (`utleggstrekkSamordnet`, eller den gamle
   ordningens `utleggstrekkSkatt`), og **betalingsmottakerens
   organisasjonsnummer og kontonummer** — brevet etter den gamle
   ordningen kan ha en annen mottaker enn den samordnede, og uten dem
   har steg 5 ingen part å føre på og betalingslisten ingen konto å
   betale til. KID-en sjekkes på kontrollsifferet som i
   `payable_terms`, kontonummeret med MOD11 som i
   `update_party_contact`, og organisasjonsnummeret med MOD11.
   **Brevet er påkrevd som vedlegg** (PDF eller skann): et hentet
   pålegg har Skatteetatens eget svar som bevis (steg 2), et manuelt
   har bare det noen skrev, og trekket blir en postering som må være
   dokumentert (bokføringsloven § 4 nr. 6). Brevet lagres **med
   pålegget**, innholdsadressert og hash-sjekket ved lesing som andre
   dokumenter (docs/blob-store.md), og kan bare leses med `LONN_LES`.
   Det festes **aldri** til lønnsbilaget: et bilagsvedlegg kan leses av
   alle som leser hovedboken, også rollen `les`, og brevet navngir den
   ansatte og gjelden (punkt 4 under Godkjenning i §3). Pålegget
   trekkes, bokføres, betales og rapporteres akkurat som et hentet og
   er merket med kilden (`manuell`, med hvem som registrerte det). Registreringen er
   innsettings-bar som de hentede versjonene: en endring eller en stopp
   er en ny rad, aldri en retting. Kommer et hentet pålegg med samme
   KID, avløser det den manuelle registreringen fra neste kjøring, og
   forhåndsvisningen sier fra. Dette dekker også trekkene etter den
   gamle ordningen (§1), som aldri kommer gjennom API-et. Har den
   ansatte både et manuelt pålegg og et hentet med **en annen** KID —
   typisk et gammelt trekk fra brev og et nytt samordnet — vises begge
   som et avvik, og kjøringen trekker det hentede: et samordnet trekk
   avslutter de andre (§1). Det manuelle står til noen med `LONN_SKRIV`
   stopper det, så stoppen er en registrert beslutning og ikke en
   slutning regnmed gjorde.
2. **Lagre ordrett.** Hver versjon lagres som den kom
   (`trekkid` + `trekkversjon` unik), innsettings-bart. Det er beviset for
   hvorfor et trekk var det det var.
3. **Knytt til den ansatte** på `skyldner` mot fødselsnummeret i
   ansattregisteret. Et pålegg uten ansatt vises som et avvik i portalen;
   det forsvinner ikke. En ansatt har bare ett samordnet pålegg (§1), og
   et erstattet pålegg kommer fra API-et som `avsluttet`. Har den ansatte
   likevel **to aktive** pålegg, viser forhåndsvisningen det som et
   avvik, og kjøringen trekker det som ble **opprettet** sist
   (`opprettet` — `sekvensnummer` stiger ved hver endring og ville
   valgt det som sist ble endret, ikke det nyeste pålegget). At
   `opprettet` hører til pålegget og ikke til versjonen, er en
   antakelse fra eksempelet som må bekreftes (§4); er den per versjon,
   brukes `opprettet` fra påleggets første versjon. Listen gir bare
   siste versjon (§1), så er versjon 1 ikke lagret, hentes den med
   `GET /api/trekkpaalegg/v1/{trekkid}/1` og lagres som enhver annen
   versjon. Advarselen navngir det andre, så selskapet kan kontakte
   innkrevingsmyndigheten.
4. **Trekk i lønnskjøringen.** For hver ansatt finner kjøringen det
   aktive pålegget og perioden som dekker utbetalingsdatoen, og beregner
   trekket. En prosent regnes av det trekkpliktige bruttobeløpet i
   kjøringen — lønn, feriepenger og trekkpliktig del av
   kjøregodtgjørelse — og aldri av trekkfri kjøregodtgjørelse, diett
   eller naturalytelser. Formatet til `trekkprosent` er heller ikke
   dokumentert (eksempelet har ingen); den leses som en desimalstreng
   til et heltall basispunkter, aldri som flyttall (slik
   `regnmed-core::currency::parse_rate` leser valutakurser), og
   trekket regnes i heltall øre med samme
   avrunding som under. Et beløp trekkes som det står — men **enheten
   til `trekkbeloep` er ikke dokumentert** (eksempelet sier 8000 uten
   enhet), og regnmed regner i øre, så en feil antakelse er en feil på
   100 ganger som taket og avrundingen ville skjult. Enheten leses fra
   OpenAPI-spesifikasjonen før koden skrives (§4). Trekket **rundes ned
   til hele kroner**, slik forskuddstrekket gjør (`regnmed-core::payroll`):
   a-meldingens `beloep` er hele kroner, og det som bokføres, betales og
   rapporteres, skal være samme tall. Lønnskjøringen er én per måned
   (unik per selskap, år og måned i `payroll_run`), så et månedsbeløp
   trekkes i sin helhet i den ene kjøringen. Trekket reduserer netto og
   vises i forhåndsvisningen og på lønnsslippen, med pålegg og versjon —
   den ansatte ser alltid sitt eget pålegg på sin egen lønnsslipp
   (arbeidsmiljøloven § 14-15 (4) krever en skriftlig oppgave over
   trekkene).
5. **Bokfør** i samme bilag som lønnen, med **én linje per pålegg**
   (lønnsbilaget er ellers samlet), siden hver har sin KID: trekket
   krediteres på
   **2610 Utleggstrekk** med innkrevingsmyndigheten
   (`betalingsmottaker`, med `kontonummer` som bankkonto) som part, i
   stedet for på 2930 Skyldig lønn. Parten slås opp blant selskapets
   **leverandører** (`kind = 'leverandor'`) på organisasjonsnummeret i
   `betalingsmottaker`, og opprettes som leverandør hvis ingen finnes.
   `party.orgnr` er verken unikt eller knyttet til typen (migrasjon
   0009), så en kunde med samme nummer telles ikke (den kan ikke føres på
   en leverandørkonto), og finnes flere leverandører, brukes den med
   lavest partsnummer, og forhåndsvisningen sier fra. Har en
   eksisterende part en annen bankkonto enn påleggets `kontonummer`,
   betaler regnmed ikke til den lagrede: betalingslisten tar
   bankkontoen fra parten (et øyeblikksbilde når kjøringen lages), og
   et trekk betalt til feil konto er arbeidsgiverens ansvar. Kjøringen
   skriver da påleggets konto på parten med samme validering som
   portalen (`update_party_contact`), men i **kjøringens egen
   transaksjon** — dagens funksjon tar en pool, så den får en
   `_in`-variant — slik at kontoendringen og lønnsbilaget lykkes eller
   feiler sammen. Forhåndsvisningen sier fra om endringen.
   2610 er en egen konto under standardkontoen 2600 «Skattetrekk og
   andre trekk», så trekket står for seg i balansen og ikke blandes med
   forskuddstrekket på 2600 eller med annen leverandørgjeld på 2400.
   Kontoen er reskontroflagget (`reskontro_kind = 'leverandor'`) og
   opprettes i selskapets kontoplan første gang en kjøring trekker et
   pålegg, med navnet «Utleggstrekk». **Finnes 2610 alt** — et selskap
   som har importert kontoplanen, kan bruke nummeret til noe annet —
   brukes den bare hvis den alt er leverandørreskontro. Ellers flagges
   den ikke om (historikken har posteringer uten part), og kjøringen
   oppretter i stedet **første ledige nummer i 2611–2619** som
   leverandørreskontro med navnet «Utleggstrekk» (SAF-T-grupperingen går
   til nærmeste standardkonto, 2600, som for 2610). Forhåndsvisningen
   sier hvilken konto som blir brukt, og kontoen huskes som
   selskapets **konto for utleggstrekk**, der selskapet kan velge en
   annen leverandørreskontrokonto. Innstillingen finnes ikke i dag og
   er ny: daterte, innsettings-bare rader per selskap (nyeste gjelder,
   som `mva_terminordning`), så det alltid kan sies hvilken konto
   trekket for en gitt måned gikk til og hvem som valgte den. Den
   endres med `LONN_SKRIV`, og kontoen sjekkes når den lagres: den må
   finnes, være aktiv og være leverandørreskontro. Kjøringen nekter
   altså ikke — lønnen skal ut, samme regel som for taket i §3 — med ett
   unntak: er ingen konto valgt, er 2610 ikke leverandørreskontro, og
   finnes det ikke noe ledig nummer i 2611–2619 (uansett hva som står
   der), finnes det ingen trygg konto å føre på. Da **nekter** kjøringen,
   og forhåndsvisningen sier det, med veien ut (velg en konto), før
   lønningsdagen. En eksisterende leverandørkonto i 2611–2619 brukes
   aldri uten at den er valgt, siden den kan bære annen gjeld. Linjen får
   betalingsvilkår i `payable_terms` med påleggets `kidnummer` og
   **forfall på utbetalingsdatoen**. Fristen er første virkedag etter
   (§1), men regnmed har ingen helligdagskalender, og ukedager alene
   tar feil rundt påske, jul og 17. mai. Forfall på utbetalingsdatoen er
   aldri for sent og krever ingen kalender.
6. **Betal** fra betalingslisten. Den betaler bare åpne
   leverandørposter og godkjente utlegg (`regnmed-db::payments`,
   migrasjon 0086), og det er derfor trekket føres som en
   leverandørpost: da står det i listen med KID og frist uten at
   betalingslisten må utvides, og oppgjøret lukker posten mot banken
   som for enhver annen leverandør. En gjeldskonto uten part ville
   aldri dukket opp der.
7. **Rapporter** trekket i a-meldingen: `utleggstrekk` med påleggets
   egen beskrivelse (`utleggstrekkSamordnet` for hentede, den
   registrerte for manuelle — `utleggstrekkSkatt` for et trekk etter den
   gamle ordningen), negativt beløp og utbetalingsdatoen, og
   `sumUtleggstrekk` over alle.

## 3. Avgjørelser

Alle avgjørelsene forslaget ventet på er tatt:

- **Godkjenning** (2026-11-25): et nytt eller endret pålegg virker fra
  **neste lønnskjøring uten godkjenning**, og **menneskene** som har
  `LONN_KJOR` varsles — de som kjører lønnen, ikke alle som kan lese den.
  `revisor` har `LONN_LES` men ikke `LONN_KJOR`, og en integrasjon kan
  ha begge (`LONN_KJOR` kan delegeres og ligger i `bokforing`), men får
  ikke varsel: en agent ser pålegget i forhåndsvisningen den uansett må
  hente før kjøringen (punkt 2 under). Pålegget er bindende, og et
  trekk som ikke gjennomføres er arbeidsgiverens ansvar
  (innledningen), så en godkjenning kunne bare utsatt trekket, aldri
  stoppet det — og utsettelsen ville vært selskapets risiko. Det samme
  gjelder motsatt vei: et stoppet eller erstattet pålegg slutter å
  trekkes uten at noen må handle. Fem ting holder automatikken trygg:
  1. Et pålegg som ikke kan knyttes til en ansatt (steg 3), vises
     **før** kjøringen, i forhåndsvisningen, ikke bare som et varsel.
  2. Har et pålegg kommet eller endret seg etter forhåndsvisningen,
     nekter kjøringen til den er forhåndsvist på nytt — det som ble vist,
     er det som kjøres. Forhåndsvisningen lagrer ingenting, så
     mekanismen er at den returnerer settet av (`trekkid`,
     `trekkversjon`) den brukte — for et manuelt registrert pålegg
     registreringens id — bokføringskallet sender settet tilbake,
     og serveren nekter hvis settet den selv ville brukt er et annet.
     Feltet er **påkrevd** når et pålegg gjelder for kjøringen: et kall
     uten settet nektes som et kall med feil sett. Lønnskjøringen er et
     offentlig API-endepunkt (docs/integrations.md), og en klient som
     ikke kjenner feltet, skal ikke kunne kjøre forbi vakten.
  3. Kjøringen advarer når siste vellykkede henting — av endringene
     eller av hellisten — er eldre enn tre dager (tre tapte daglige
     hentinger, steg 1), siden den da kan trekke etter pålegg som er
     stoppet eller erstattet. Har selskapet
     aldri hentet, gjelder advarselen i steg 1.
  4. Andres pålegg, varselet og koblingen til den ansatte er synlige
     bare for dem med `LONN_LES`: gjeld hos en ansatt er sensitive
     opplysninger. Den ansatte ser sitt eget pålegg på lønnsslippen
     (steg 4). Bokføringen kan ikke skjules på samme måte: hvert
     trekk er en egen åpen post på 2610 med påleggets KID (steg 5–6),
     så den som kan lese hovedboken, leverandørreskontroen eller
     betalingslisten — også rollen `les`, som ellers ikke ser lønn — ser
     ett beløp per pålegg. Linjeteksten navngir derfor aldri den
     ansatte («Utleggstrekk 07/2026»), og koblingen fra KID til person
     finnes bare i trekkpåleggstabellen bak `LONN_LES`.
  5. Et pålegg virker ikke bakover: kommer det etter at månedens lønn
     er kjørt, trekkes det fra neste kjøring. Om Skatteetaten regner
     virkningstidspunktet fra mottak eller fra utbetalingsdato, er ikke
     funnet i veiledningen og må bekreftes i testmiljøet.
- **Gjeldskontoen** (2026-11-25): en egen konto, 2610 Utleggstrekk,
  framfor 2400 Leverandørgjeld, så trekket er synlig for seg i balansen
  (steg 5).
- **Trekk større enn netto** (2026-11-25): kjøringen trekker det som er
  igjen etter forskuddstrekket **av lønnen**, aldri mer, og den
  ansattes trekkfrie kjøregodtgjørelse og diett røres ikke — de er
  utgiftsgodtgjørelser som holdes utenfor (§1), selv om de utbetales
  med lønnen og regnes inn i `net_ore`. Taket er altså netto minus
  trekkfrie godtgjørelser, og den delen av netto blir null, aldri
  negativ. Kjøringen **advarer** med påleggets beløp og det som faktisk
  ble trukket — i forhåndsvisningen og i kjøringens resultat. Kjøringen
  stopper ikke: lønnen skal ut, og forskuddstrekket går foran. Det som
  ble trukket, er det som bokføres og rapporteres i a-meldingen;
  differansen føres ikke over til neste måned av regnmed. Er årsaken
  ikke synlig i a-meldingen, sier advarselen at selskapet må kontakte
  innkrevingsmyndigheten. Dette er Skatteetatens egen regel («trekke så
  mye som mulig», §1).
- **Grunnlaget for prosenten, fristen, a-meldingens felt, flere pålegg
  og fratrådte** (2026-11-25): avgjort av Skatteetatens regler i §1, ikke
  av oss. Den samordnede ordningen har ett pålegg per ansatt, men i
  overgangen fra den gamle kan en ansatt ha et manuelt registrert trekk
  fra brev ved siden av et hentet samordnet; steg 1 sier hva som
  trekkes da. En fratrådt meldes i a-meldingen.

## 4. Før noe kan prøves mot Skatteetaten

1. Test-scopet `skatteetaten:trekkpaalegg` er bestilt for BogenTech
   (e-post til fremtidensinnkreving@skatteetaten.no) og lagt på
   testklienten i Samarbeidsportalen.
2. regnmeds system i Altinns systemregister (TT02) har fått ressursen
   `ske-informasjon-om-trekkpaalegg`, og testselskapets systembruker er
   godkjent på nytt med den.
3. Skatteetaten har lagt inn trekkpålegg i testmiljøet for det
   syntetiske selskapet.
4. OpenAPI-spesifikasjonen er lest, og dette står i dokumentet med
   kilde: enheten til `trekkbeloep` (kroner eller øre), formatet til
   `trekkprosent`, om `fraSekvensnummer` er inkluderende, om `opprettet`
   gjelder pålegget eller versjonen, og om et stoppet pålegg kommer i
   endringslisten som `avsluttet` eller bare forsvinner.

## 5. Ferdig når

Et trekkpålegg hentet fra Skatteetatens testmiljø blir trukket i en
lønnskjøring, bokført, betalt fra betalingslisten og rapportert i en
a-melding som Skatteetaten tar imot uten avvik. Samme beviskrav som for
a-meldingen og skattekortet i docs/lonn.md.
