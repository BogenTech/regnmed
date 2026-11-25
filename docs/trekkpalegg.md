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
betalingsinformasjonen. Hvilke verdier `beskrivelse` kan ha, står i
a-meldingens kodeliste og er **ikke sjekket ennå**.

## 2. Flyten i regnmed

1. **Hent.** En jobb i arbeideren henter `GET /api/trekkpaalegg/v1` for
   hvert selskap som har systembruker og har levert minst én a-melding
   gjennom regnmed, med `fraSekvensnummer` fra forrige vannmerke. Samme
   kall kan startes fra portalen («Hent trekkpålegg»). Som for
   skattekortet skrives forespørselen **før** kallet, og en feil lagres
   med koden.
2. **Lagre ordrett.** Hver versjon lagres som den kom
   (`trekkid` + `trekkversjon` unik), innsettings-bart. Det er beviset for
   hvorfor et trekk var det det var.
3. **Knytt til den ansatte** på `skyldner` mot fødselsnummeret i
   ansattregisteret. Et pålegg uten ansatt vises som et avvik i portalen;
   det forsvinner ikke.
4. **Trekk i lønnskjøringen.** For hver ansatt finner kjøringen de
   aktive påleggene, perioden som dekker utbetalingsdatoen, og beregner
   trekket: prosent av bruttolønnen, eller beløpet. Trekket reduserer
   netto og vises i forhåndsvisningen og på lønnsslippen, med
   pålegg og versjon.
5. **Bokfør** i samme bilag som lønnen: kredit på en gjeldskonto for
   utleggstrekk i stedet for 2930 Skyldig lønn.
6. **Betal** fra betalingslisten til `kontonummer` med `kidnummer`, så
   gjelden går i null mot banken som annen gjeld.
7. **Rapporter** trekket i a-meldingen (`utleggstrekk` og
   `sumUtleggstrekk`).

## 3. Det som må avgjøres før koden skrives

- **Gjeldskontoen.** Hvilken konto i 2xxx trekket føres på.
- **Trekk større enn netto.** Hva kjøringen skal gjøre når trekket er
  større enn det som er igjen etter forskuddstrekk.
- **«Brutto inntekt».** Om prosenten regnes av hele bruttoen i kjøringen
  — med feriepenger — eller av lønnen alene, og om trekkfri
  kjøregodtgjørelse og diett holdes utenfor.
- **Frist for innbetaling.** Når trekket må være betalt til
  innkrevingsmyndigheten. Står ikke i API-dokumentasjonen.
- **A-meldingens `beskrivelse`.** Hvilken kodeverdi som brukes.
- **Godkjenning.** Om et nytt pålegg skal virke fra neste kjøring uten
  videre, eller om en person i selskapet først skal se det. Pålegget er
  bindende for arbeidsgiveren, så trolig uten videre, men med varsel.
- **Fratrådte.** Et pålegg på en som ikke lenger er ansatt.

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
