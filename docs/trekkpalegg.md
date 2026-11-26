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
  beskjed om å stoppe dem; de er utenfor regnmed, og merknaden i
  forhåndsvisningen av lønnskjøringen gjelder fortsatt for dem.
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
   idempotent — en versjon som alt finnes (`on conflict do nothing`),
   hoppes over, og er innholdet en annen enn det lagrede, stopper
   hentingen med en feil i stedet for å skrive over. Ellers ville én
   gjentatt versjon rulle tilbake hele hentingen, også de nye, hver dag.
   Samme kall kan startes fra portalen («Hent trekkpålegg»). Som for skattekortet skrives forespørselen **før**
   kallet, og en feil lagres med koden.

   Kravet om en levert a-melding er API-ets, ikke vårt: et selskap som
   kommer til regnmed med et løpende pålegg, får det ikke før etter
   første a-melding herfra. Har selskapet aldri hentet med hell, sier
   derfor forhåndsvisningen av kjøringen at eventuelle pålegg selskapet
   alt har, ikke er med, og må trekkes og betales utenfor regnmed den
   måneden.
2. **Lagre ordrett.** Hver versjon lagres som den kom
   (`trekkid` + `trekkversjon` unik), innsettings-bart. Det er beviset for
   hvorfor et trekk var det det var.
3. **Knytt til den ansatte** på `skyldner` mot fødselsnummeret i
   ansattregisteret. Et pålegg uten ansatt vises som et avvik i portalen;
   det forsvinner ikke. En ansatt har bare ett samordnet pålegg (§1), og
   et erstattet pålegg kommer fra API-et som `avsluttet`. Har den ansatte
   likevel **to aktive** pålegg, velger regnmed ikke mellom dem:
   forhåndsvisningen viser det som et avvik, og kjøringen trekker det med
   høyest `sekvensnummer` og advarer om det andre, så selskapet kan
   kontakte innkrevingsmyndigheten.
4. **Trekk i lønnskjøringen.** For hver ansatt finner kjøringen det
   aktive pålegget og perioden som dekker utbetalingsdatoen, og beregner
   trekket. En prosent regnes av det trekkpliktige bruttobeløpet i
   kjøringen — lønn, feriepenger og trekkpliktig del av
   kjøregodtgjørelse — og aldri av trekkfri kjøregodtgjørelse, diett
   eller naturalytelser. Et beløp trekkes som det står — men **enheten
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
   stedet for på 2930 Skyldig lønn. Parten slås opp på
   organisasjonsnummeret i `betalingsmottaker` og opprettes som
   leverandør hvis den ikke finnes. Har en eksisterende part en annen
   bankkonto enn påleggets `kontonummer`, betaler regnmed ikke til den
   lagrede: betalingslisten tar bankkontoen fra parten (et øyeblikksbilde
   når kjøringen lages), og et trekk betalt til feil konto er
   arbeidsgiverens ansvar. Kjøringen skriver da påleggets konto på parten
   gjennom samme vei som portalen (`update_party_contact`, som validerer
   kontonummeret), og forhåndsvisningen sier fra om endringen.
   2610 er en egen konto under standardkontoen 2600 «Skattetrekk og andre trekk», så trekket står
   for seg i balansen og ikke blandes med forskuddstrekket på 2600 eller
   med annen leverandørgjeld på 2400. Kontoen er reskontroflagget
   (`reskontro_kind = 'leverandor'`) og opprettes i selskapets kontoplan
   første gang en kjøring trekker et pålegg, med navnet
   «Utleggstrekk». **Finnes 2610 alt** — et selskap som har importert
   kontoplanen, kan bruke nummeret til noe annet — brukes den bare hvis
   den alt er leverandørreskontro. Ellers flagges den ikke om (historikken
   har posteringer uten part); forhåndsvisningen sier fra før kjøringen,
   og selskapet velger en annen konto for utleggstrekk i lønnsinnstillingene
   (standard 2610), som må være en leverandørreskontrokonto. Kjøringen
   nekter bare når en ansatt faktisk har et pålegg å trekke. Linjen får
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
7. **Rapporter** trekket i a-meldingen: `utleggstrekk` med
   `utleggstrekkSamordnet`, negativt beløp og utbetalingsdatoen, og
   `sumUtleggstrekk`.

## 3. Avgjørelser

Alle avgjørelsene forslaget ventet på er tatt:

- **Godkjenning** (2026-11-25): et nytt eller endret pålegg virker fra
  **neste lønnskjøring uten godkjenning**, og de som har `LONN_KJOR`
  varsles — de som kjører lønnen, ikke alle som kan lese den (`revisor`
  og integrasjoner kan ha `LONN_LES`; de leser pålegget, men får ikke
  varsel). Pålegget er bindende, og et trekk som ikke gjennomføres er
  arbeidsgiverens ansvar (innledningen), så en godkjenning kunne bare
  utsatt trekket, aldri stoppet det — og utsettelsen ville vært
  selskapets risiko. Det samme gjelder motsatt vei: et stoppet eller
  erstattet pålegg slutter å trekkes uten at noen må handle. Fem ting
  holder automatikken trygg:
  1. Et pålegg som ikke kan knyttes til en ansatt (steg 3), vises
     **før** kjøringen, i forhåndsvisningen, ikke bare som et varsel.
  2. Har et pålegg kommet eller endret seg etter forhåndsvisningen,
     nekter kjøringen til den er forhåndsvist på nytt — det som ble vist,
     er det som kjøres. Forhåndsvisningen lagrer ingenting, så
     mekanismen er at den returnerer settet av (`trekkid`,
     `trekkversjon`) den brukte, bokføringskallet sender settet tilbake,
     og serveren nekter hvis settet den selv ville brukt er et annet.
     Feltet er **påkrevd** når et pålegg gjelder for kjøringen: et kall
     uten settet nektes som et kall med feil sett. Lønnskjøringen er et
     offentlig API-endepunkt (docs/integrations.md), og en klient som
     ikke kjenner feltet, skal ikke kunne kjøre forbi vakten.
  3. Kjøringen advarer når siste vellykkede henting er eldre enn tre
     dager — tre tapte daglige hentinger (steg 1) — siden den da kan
     trekke etter pålegg som er stoppet eller erstattet. Har selskapet
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
  av oss. Flere pålegg per ansatt finnes ikke i den samordnede ordningen,
  og en fratrådt meldes i a-meldingen.

## 4. Før noe kan prøves mot Skatteetaten

1. Test-scopet `skatteetaten:trekkpaalegg` er bestilt for BogenTech
   (e-post til fremtidensinnkreving@skatteetaten.no) og lagt på
   testklienten i Samarbeidsportalen.
2. regnmeds system i Altinns systemregister (TT02) har fått ressursen
   `ske-informasjon-om-trekkpaalegg`, og testselskapets systembruker er
   godkjent på nytt med den.
3. Skatteetaten har lagt inn trekkpålegg i testmiljøet for det
   syntetiske selskapet.
4. OpenAPI-spesifikasjonen er lest, og enheten til `trekkbeloep` (kroner
   eller øre) og om `fraSekvensnummer` er inkluderende, står i dette
   dokumentet med kilde.

## 5. Ferdig når

Et trekkpålegg hentet fra Skatteetatens testmiljø blir trukket i en
lønnskjøring, bokført, betalt fra betalingslisten og rapportert i en
a-melding som Skatteetaten tar imot uten avvik. Samme beviskrav som for
a-meldingen og skattekortet i docs/lonn.md.
