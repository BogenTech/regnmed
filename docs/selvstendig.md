# Betalinger til selvstendig næringsdrivende (tidligere RF-1321)

Tredjepartsopplysninger til Skatteetaten om hva en næringsdrivende har
kjøpt av tjenester fra selvstendig næringsdrivende **uten fast
forretningssted**. Ett beløp per mottaker per inntektsår, levert innen
**15. februar** året etter. Den første som betyr noe for kundene er
15. februar 2027, for inntektsåret 2026. Issue #363; kopien til mottakeren
er #364.

## Regelverket

Kilder: skatteforvaltningsloven § 7-10 (1) bokstav e og (2), § 7-14,
skatteforvaltningsforskriften §§ 7-10-23 til 7-10-25 og § 7-13-1, og
Skatteetatens rettledning («Betalinger til selvstendig næringsdrivende»,
skatteetaten.no, lest 2026-11-28).

- **Hvem leverer.** Den som har mottatt krav på vederlag for tjenester av
  teknisk, håndverksmessig, juridisk, regnskapsmessig eller annen art
  (forskriften § 7-10-23 (1)). Unntatt er oppdragsgiver uten fradragsrett
  (§ 7-10-23 (2)).
- **Hvem rapporteres.** Selvstendig næringsdrivende som **ikke** driver fra
  fast forretningssted i Norge (§ 7-10-24 (2)). Aksjeselskap og ansvarlig
  selskap rapporteres aldri. Om mottakeren har fast forretningssted, avgjør
  oppdragsgiveren selv, og rettledningen sier at **ved tvil skal det
  rapporteres**. Registrering i Enhetsregisteret eller
  Merverdiavgiftsregisteret avgjør ikke spørsmålet.
- **Beløpet.** «Samlet fakturert beløp eksklusive merverdiavgift» i året
  (§ 7-10-24 (1)). Kreditnotaer trekker fra. Varer levert sammen med
  tjenesten er med, rene varekjøp ikke. Det er ingen nedre grense. Er netto
  0, rapporteres ingenting. Beløpet oppgis i hele kroner, vanlig avrundet.
  API-feltet heter `betaltBeloep`, men forskriften sier fakturert, og
  forskriften gjelder.
- **Identifikator.** Mottakerens organisasjonsnummer når den har ett.
  Ellers fødselsnummer eller D-nummer (§ 7-14 (1)).
- **Korrigering.** Skatteetaten erstatter en oppgave per (inntektsår,
  oppgavegiver, mottaker). En oppgave slettes ved å sende den på nytt med
  beløp 0.
- **Kopi til mottakeren.** Mottakeren skal ha en kopi innen 15. februar
  som sier at opplysningene også er sendt Skatteetaten (§ 7-14 (3),
  § 7-13-1). Ikke bygget ennå: #364.
- **Nulloppgave.** Ingen plikt funnet. API-et tar `ingenoppgaver`, men
  regnmed sender aldri en.

## Avgjort 2026-11-28

Bekreftet av brukeren før designet ble skrevet:

1. **Beløpet er fakturert eks. mva**, netto etter kreditnotaer, avrundet
   til hele kroner. Netto 0 eller lavere gir ingen oppgave.
2. **Ikke vurderte leverandører gir advarsel, ikke stopp.** De listes på
   oppgavesiden og navngis i bekreftelsen før innsending, men hindrer ikke
   levering.
3. **Identifikator:** organisasjonsnummer, ellers fødsels- eller D-nummer
   lagret på vurderingen og vist maskert. En «ja»-vurdering uten noen av
   dem hindrer levering.
4. **Vurderingen gjelder hele leverandøren.** Det finnes ingen unntak per
   faktura i v1 (husleie, persontransport, rene varekjøp). Det er kundens
   ansvar å vurdere slik at bare tjenestekjøp blir med.
5. **Kopien til mottakeren** er en egen sak (#364), som skal være på plass
   før 15. februar 2027.

## Hva regnmed vet, og hva kunden må si

**Leverandørreskontroen** har det meste: hver leverandør er en part med
navn og eventuelt organisasjonsnummer, og hver faktura og kreditnota er
bokført med parten på leverandørgjeldskontoen (docs/reskontro.md).

**Kunden må si hvem som er selvstendig næringsdrivende uten fast
forretningssted.** Det er en skjønnsvurdering lovgiveren legger på
oppdragsgiveren, og regnmed gjetter den aldri. Den registreres per
leverandør i `contractor_assessment` (migrasjon 0112) som ja eller nei,
med hvem som sa det og når. Tabellen er bare innsetting, og nyeste rad
gjelder. Fødselsnummeret lagres bare for en leverandør uten
organisasjonsnummer, og vises aldri i portalen eller i API-ets
forhåndsvisning (bare fødselsdatoen). Det står bare i filen som leveres.

Tre utfall per leverandør med fakturaer i året:

| Vurdering | I oppgaven |
| --- | --- |
| Ja | Ja, med beløpet. Mangler identifikator: oppgaven kan ikke leveres |
| Nei | Nei |
| Ikke vurdert | Nei, men oppført som **ikke vurdert** på siden og i bekreftelsen |

## Tallene

`regnmed-db::contractor_payments::report` leser leverandørreskontroen for
året (bilagsdato i året):

- En postering på en leverandørkonto (`reskontro_kind = 'leverandor'`) er
  en **faktura** når den krediterer leverandøren og en **kreditnota** når
  den debiterer. Unntaket er når penger gikk motsatt vei i samme bilag
  (konto 19xx). Da er den en **betaling** eller en **tilbakebetaling**, og
  telles ikke. En faktura som bokføres og betales i samme bilag, telles
  derfor riktig.
- **Mva** er bilagets egen på 2700–2719. Den deles forholdsmessig hvis
  bilaget har flere leverandører (`regnmed-core::contractor_payments::invoiced_excl_vat`).
  Omvendt avgiftsplikt går i null og trekker ingenting fra.
- Det avrundes til hele kroner per mottaker (50 øre opp), og summen i
  oppsummeringen er summen av de avrundede beløpene.

**Grensene, ærlig sagt:**

- Det som er bokført **uten leverandør**, for eksempel en kostnad ført
  rett fra banken eller et utlegg, ser ikke regnmed. Kunden må føre slike
  kjøp mot leverandørreskontroen for at de skal komme med.
- Vurderingen gjelder hele leverandøren. Kjøper kunden både tjenester og
  rene varer (eller husleie) fra samme selvstendige, kommer alt med.
- Mva leses fra kontoene 2700–2719. Mva bokført på en annen konto blir
  stående i beløpet.
- En fil kunden har lastet ned og levert selv, vet regnmed ikke om. En
  korrigering trekker bare tilbake det **regnmed** har levert.

## Filen

`regnmed-core::contractor_payments::render` lager `Melding`-dokumentet
(JSON) i Skatteetatens OpenAPI 1.0.0, som ligger vendored i
[selvstendig/](selvstendig/). Enhetstestene validerer dokumentet mot
skjemaet med en ekte JSON Schema-validator (`boon`, bare i tester), og
viser at validatoren avviser det Skatteetaten ville avvist: ukjent
leveransetype, negativt beløp, både fødselsnummer og orgnr på samme
mottaker, manglende inntektsår.

- `kildesystem` er `regnmed`. `kontaktinformasjon` er selskapets navn og
  e-post, ikke personens, slik at et nytt forsøk av en annen person gir
  samme bytes.
- `oppgavegiversLeveranseReferanse` er innsendingens id. En nedlastet fil
  får en ny referanse hver gang.

**Last ned** (`GET …/reports/selvstendig?year=&format=fil`, i portalen
under Leverandører → Betalinger til selvstendig næringsdrivende) gir
filen, slik at kunden kan levere uten noen tilgang fra oss.

## Innsending

`POST https://innrapporteringbetalingernaeringsdrivende.api.skatteetaten.no/v1/{inntektsaar}`
med Maskinporten-token for kundens **Altinn-systembruker** (scope
`skatteetaten:innrapporteringbetalingerselvstendignaeringsdrivende`,
Altinn-ressurs `ske-innrapportering-betalinger-selvstendig-naeringsdrivende`)
og en `idempotencyKey`. Svaret er en `Leveransebekreftelse` (`dialogId`,
`forsendelseId`). Det er bevis på levering, ikke på godkjenning.
Testmiljøet er standard (`CONTRACTOR_PAYMENTS_URL` velger et annet), som
for RF-1086.

Samme skinne som a-meldingen og RF-1086:

- **Kundevilkårene først.** Både API-et (`terms::require_accepted`) og
  databasefunksjonen (`terms::may_file`) nekter før selskapet har godtatt
  vilkårene i kraft.
- **Lagret før sendt** (migrasjon 0112, regelen fra 0076). Dokumentet og
  nøkkelen skrives før kallet. Et forsøk uten kvittering sendes på nytt med
  samme bytes og samme nøkkel så lenge tallene er de samme, og leveres
  aldri to ganger. Endres tallene, blir det en ny innsending.
- **Korrigering.** En levert oppgave som er lik dagens, sendes ikke igjen.
  Er noe endret, sendes hele settet på nytt. En mottaker den forrige
  leveransen hadde, men som ikke er med nå, sendes med 0 kr, som er
  Skatteetatens måte å slette en oppgave på.
- **Svaret** hentes fra Dialogporten med «Hent svar» (samme kode som
  RF-1086). Det kommer ikke av seg selv ennå, fordi det ikke finnes noe
  Altinn-abonnement for denne ressursen.
- **Registeret.** Hver innsending står i registeret over kall til
  myndighetene (`terms::authority_calls`, skjema `betalinger-selvstendig`).

## Rettigheter

| Hva | Rettighet |
| --- | --- |
| Se vurderingen, oppgaven og innsendingene, laste ned filen | `RESKONTRO_LES` |
| Vurdere en leverandør, sende inn | `RESKONTRO_SKRIV` |

Filen bærer fødselsnumre, akkurat som RF-1086-filen bak aksjebokens
lesere. Eksport sperres ikke av et sperret abonnement.

## Ikke bevist ennå

**Ingenting er sendt til Skatteetaten.** Test-scopet ble bestilt
2026-11-27 (SSV-6691) og er ikke tildelt. Klienten er testet mot et
axum-mock som svarer med dokumentasjonens egen `Leveransebekreftelse`
([leveransebekreftelse-eksempel.json](selvstendig/leveransebekreftelse-eksempel.json)).
Feilsvaret ([feil-eksempel.json](selvstendig/feil-eksempel.json)) er laget
etter skjemaets `Feil` og feilkodetabellen, ikke tatt opp. Når scopet er
tildelt, må dette bevises mot testmiljøet:

- at kallet tas imot med systembrukertoken for testselskapet 311032240;
- at svaret kommer som en transmisjon i Dialogporten knyttet til vår
  `forsendelseId`, slik det gjør for RF-1086;
- at en korrigering med 0 kr sletter oppgaven;
- at et D-nummer tas imot i feltet `foedselsnummer` (skjemaet har ikke
  eget felt).

**Systembrukeren må bære ressursen.** Ressursen står i systemets `rights`
(`scripts/altinn-system.py`), men ikke i forespørselen kundene godkjenner
(`regnmed-gov::altinn_system_user::RESOURCES`). Kunder som allerede har
godkjent regnmed, må godkjenne en ny forespørsel før innsending virker.
Dette legges til når scopet er tildelt og systemet er oppdatert i Altinn,
på samme måte som for trekkpålegg.

**Versjon 1.1.0** av OpenAPI-en (upublisert, endret 2026-11-25) legger til
`POST /v2/{inntektsaar}` med `innsenderKontaktinformasjon` (navn og e-post
påkrevd) rundt samme `Melding`. Vi bygger mot den publiserte 1.0.0 og
følger med.
