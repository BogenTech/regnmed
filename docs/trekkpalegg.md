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

### A-meldingen

A-meldingen v2.3 (vendored i docs/amelding/) har `utleggstrekk` per
inntektsmottaker — `beskrivelse`, `beloep` (hele kroner),
`datoForUtleggstrekk` (valgfri) — og `sumUtleggstrekk` i
betalingsinformasjonen. `beskrivelse` er fri tekst i skjemaet
(`Utleggstrekkbeskrivelse` er en ubegrenset `xsd:string`); hva som skal
stå der, må komme fra Skatteetatens veiledning til a-meldingen og er
**ikke sjekket ennå**.

## 2. Flyten i regnmed

1. **Hent.** En jobb i arbeideren henter `GET /api/trekkpaalegg/v1` for
   hvert selskap som har systembruker og har levert minst én a-melding
   gjennom regnmed, med `fraSekvensnummer` fra forrige vannmerke. Den
   henter side for side til en side kommer tilbake med færre enn
   `maksAntall` — én side kan ikke antas å være alt. Vannmerket flyttes
   til høyeste `sekvensnummer` i **samme transaksjon** som versjonene
   lagres, så et kall som feiler halvveis hentes på nytt neste gang i
   stedet for å hoppes over. Samme kall kan startes fra portalen («Hent
   trekkpålegg»). Som for skattekortet skrives forespørselen **før**
   kallet, og en feil lagres med koden.
2. **Lagre ordrett.** Hver versjon lagres som den kom
   (`trekkid` + `trekkversjon` unik), innsettings-bart. Det er beviset for
   hvorfor et trekk var det det var.
3. **Knytt til den ansatte** på `skyldner` mot fødselsnummeret i
   ansattregisteret. Et pålegg uten ansatt vises som et avvik i portalen;
   det forsvinner ikke.
4. **Trekk i lønnskjøringen.** For hver ansatt finner kjøringen de
   aktive påleggene, perioden som dekker utbetalingsdatoen, og beregner
   trekket: prosent av grunnlaget (se «Brutto inntekt» i §3), eller
   beløpet. Lønnskjøringen er én per måned (unik per selskap, år og
   måned i `payroll_run`), så et månedsbeløp trekkes i sin helhet i den
   ene kjøringen. Trekket reduserer
   netto og vises i forhåndsvisningen og på lønnsslippen, med
   pålegg og versjon.
5. **Bokfør** i samme bilag som lønnen: trekket krediteres på
   **2610 Utleggstrekk** med innkrevingsmyndigheten
   (`betalingsmottaker`, med `kontonummer` som bankkonto) som part, i
   stedet for på 2930 Skyldig lønn. 2610 er en egen konto under
   standardkontoen 2600 «Skattetrekk og andre trekk», så trekket står
   for seg i balansen og ikke blandes med forskuddstrekket på 2600 eller
   med annen leverandørgjeld på 2400. Kontoen er reskontroflagget
   (`reskontro_kind = 'leverandor'`) og opprettes i selskapets kontoplan
   første gang en kjøring trekker et pålegg, med navnet
   «Utleggstrekk». Linjen får betalingsvilkår i
   `payable_terms` med påleggets `kidnummer` og fristen for innbetaling.
6. **Betal** fra betalingslisten. Den betaler bare åpne
   leverandørposter og godkjente utlegg (`regnmed-db::payments`,
   migrasjon 0086), og det er derfor trekket føres som en
   leverandørpost: da står det i listen med KID og frist uten at
   betalingslisten må utvides, og oppgjøret lukker posten mot banken
   som for enhver annen leverandør. En gjeldskonto uten part ville
   aldri dukket opp der.
7. **Rapporter** trekket i a-meldingen (`utleggstrekk` og
   `sumUtleggstrekk`).

## 3. Det som må avgjøres før koden skrives

- **Flere pålegg som ikke får plass.** Har en ansatt flere pålegg og
  netto ikke dekker alle, i hvilken rekkefølge de trekkes.
- **«Brutto inntekt».** Om prosenten regnes av hele bruttoen i kjøringen
  — med feriepenger — eller av lønnen alene, og om trekkfri
  kjøregodtgjørelse og diett holdes utenfor.
- **Frist for innbetaling.** Når trekket må være betalt til
  innkrevingsmyndigheten. Står ikke i API-dokumentasjonen.
- **A-meldingens `beskrivelse`.** Hvilken tekst feltet skal ha — for
  eksempel saksnummeret — etter Skatteetatens veiledning; skjemaet
  tillater hva som helst.
- **Godkjenning.** Om et nytt pålegg skal virke fra neste kjøring uten
  videre, eller om en person i selskapet først skal se det. Pålegget er
  bindende for arbeidsgiveren, så trolig uten videre, men med varsel.
- **Fratrådte.** Et pålegg på en som ikke lenger er ansatt.

Avgjort:

- **Gjeldskontoen** (2026-11-25): en egen konto, 2610 Utleggstrekk,
  framfor 2400 Leverandørgjeld, så trekket er synlig for seg i balansen
  (steg 5).
- **Trekk større enn netto** (2026-11-25): kjøringen trekker det som er
  igjen etter forskuddstrekket, aldri mer, så netto blir null og aldri
  negativ, og den **advarer** med påleggets beløp og det som faktisk
  ble trukket — i forhåndsvisningen og i kjøringens resultat. Kjøringen
  stopper ikke: lønnen skal ut, og forskuddstrekket går foran. Det som
  ble trukket, er det som bokføres og rapporteres i a-meldingen;
  differansen føres ikke over til neste måned av regnmed.

## 4. Før noe kan prøves mot Skatteetaten

1. Test-scopet `skatteetaten:trekkpaalegg` er bestilt for BogenTech
   (e-post til fremtidensinnkreving@skatteetaten.no) og lagt på
   testklienten i Samarbeidsportalen.
2. regnmeds system i Altinns systemregister (TT02) har fått ressursen
   `ske-informasjon-om-trekkpaalegg`, og testselskapets systembruker er
   godkjent på nytt med den.
3. Skatteetaten har lagt inn trekkpålegg i testmiljøet for det
   syntetiske selskapet.

## 5. Ferdig når

Et trekkpålegg hentet fra Skatteetatens testmiljø blir trukket i en
lønnskjøring, bokført, betalt fra betalingslisten og rapportert i en
a-melding som Skatteetaten tar imot uten avvik. Samme beviskrav som for
a-meldingen og skattekortet i docs/lonn.md.
