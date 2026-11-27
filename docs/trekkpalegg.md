# Trekkpålegg

**Status** (2026-11-27): pålegg **registrert for hånd** (§6, #339) er
bygget — de trekkes, bokføres, betales fra betalingslisten og
rapporteres i a-meldingen. **Henting fra Skatteetaten** (§2 steg 1–3,
#334) er ikke bygget; den venter på scopet (SSV-6690). Det felles
kjernen §6 og #334 deler — kontoen, trekket med taket, bokføringen,
a-meldingsfeltene og forhåndsvisningens sett — er bygget slik at
hentede pålegg kan kobles på som flere rader av samme slag
(`regnmed-db::garnishment::Effective`).

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
OpenAPI-spesifikasjonen (SwaggerHub `skatteetaten/trekkpaalegg-app`,
versjon 1.4, åpent lesbar) er lest 2026-11-26; der den sier noe
dokumentasjonen ikke gjør, står det under med «spesifikasjonen».

### Tilgang

- Scope `skatteetaten:trekkpaalegg`, med **systembruker** når vi henter
  for en kunde. Systembrukerforespørselen må da også ha ressursen
  `ske-informasjon-om-trekkpaalegg` (tilgangsressursen «Informasjon om
  trekkpålegg»).
- Dokumentasjonen sier at test-scopet bestilles med e-post til
  fremtidensinnkreving@skatteetaten.no, men brukerstøttedesken har et
  eget skjema for det (Hente data → Søknad om tilgang, docs/gov.md), og
  det er der det er bestilt (SSV-6690). Testdata (trekkpålegg på
  syntetiske personer i et syntetisk selskap) bes om når integrasjonen
  når tjenesten — på samme sak, eller på adressen over.
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
| GET | `/api/trekkpaalegg/v1` | Siste versjon av **samtlige** trekkpålegg for den trekkpliktige (spesifikasjonen), hvert med `trekkstatus` `aktiv` eller `avsluttet`. |
| GET | `/api/trekkpaalegg/v1/{trekkid}/{trekkversjon}` | Én bestemt versjon. |

`fraSekvensnummer` + `maksAntall` gir bare pålegg «nyere enn» et
sekvensnummer (eksklusivt), og spesifikasjonen anbefaler dem bare for
trekkpliktige med «svært mange (flere tusen)» pålegg. Et SMB har en
håndfull, så regnmed bruker dem ikke (§2, steg 1).

Test: `https://api-test.sits.no`. Produksjon:
`https://api.skatteetaten.no/api/trekkpaalegg/v1`. Svaret er JSON.
Skatteetaten sier at SBS må **polle**; en hendelse via Dialogporten skal
komme senere. Dokumentasjonen oppgir feilkodene `KB-001`–`KB-009`,
mens spesifikasjonens eksempel er `TKL-0xx`; regnmed lagrer koden som
den kommer og matcher ikke på prefikset.

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
- Begge kan være 0. Begge er `Desimaltall` (`number`, `double`) i
  spesifikasjonen: `trekkbeloep` er altså **kroner** med desimaler —
  et beløp i øre ville vært et heltall — og `trekkprosent` en
  desimalprosent.
- **`trekkstatus: avsluttet`**: det skal ikke lenger trekkes.
- `trekkid` er stabil, `trekkversjon` øker med 1, og `sekvensnummer` er
  et globalt, alltid stigende løpenummer. `opprettet` er et tidspunkt
  uten forklaring; om det gjelder pålegget eller versjonen, sier verken
  dokumentasjonen eller spesifikasjonen (§4).
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
  registreres for hånd (§6, #339).
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

1. **Hent.** En jobb i arbeideren henter **hele listen**,
   `GET /api/trekkpaalegg/v1` uten `fraSekvensnummer`, **daglig** for
   hvert selskap som har systembruker og har levert minst én a-melding
   gjennom regnmed. Listen gir siste versjon av samtlige pålegg med
   status (§1), og et selskap har en håndfull, så det er ingen
   vannmerke å holde rede på og ingen sider å bla gjennom. En versjon
   (`trekkid`, `trekkversjon`) regnmed ikke har, lagres; en den alt
   har, hoppes over. Samme henting kan startes fra portalen («Hent
   trekkpålegg»). Som for skattekortet skrives forespørselen **før**
   kallet, og en feil lagres med koden.

   Et pålegg som er aktivt i regnmed, men ikke er med i siste
   vellykkede liste, trekkes **ikke**: det vises som et avvik, og
   advarselen ber selskapet kontakte innkrevingsmyndigheten. Listen skal
   ha med også avsluttede pålegg, så dette skal ikke skje — men skjer
   det, er det tryggere å la være å trekke enn å trekke uten grunnlag.

   Forhåndsvisningen og bokføringen leser begge det lagrede, aldri
   Skatteetaten direkte: forhåndsvisningen lagrer ingenting
   (docs/lonn.md), og de to må regne ut samme sett (punkt 2 under
   Godkjenning i §3).

   **Ikke dekket:** API-et gir pålegg først etter selskapets første
   a-melding gjennom regnmed, og trekk etter den gamle ordningen kommer
   bare som brev (§1). Begge deler dekkes av pålegg registrert for hånd
   (§6, #339). Til det er bygget, sier forhåndsvisningen av kjøringen at
   slike pålegg ikke er med, og at regnmed ikke kan rapportere et trekk
   som er gjort utenfor: a-meldingen fra regnmed er da ufullstendig for
   den ansatte den måneden (#339).
2. **Lagre ordrett.** Hver versjon lagres som den kom
   (`trekkid` + `trekkversjon` unik), innsettings-bart. Det er beviset for
   hvorfor et trekk var det det var.
3. **Knytt til den ansatte** på `skyldner` mot fødselsnummeret i
   ansattregisteret. Et pålegg uten ansatt vises som et avvik i portalen;
   det forsvinner ikke. En ansatt har bare ett samordnet pålegg (§1), og
   et erstattet pålegg kommer som `avsluttet`. Har den ansatte likevel
   **to aktive** pålegg, viser forhåndsvisningen det som et avvik, og
   kjøringen trekker det med senest `opprettet` (ikke høyest
   `sekvensnummer`, som stiger ved hver endring). Er `opprettet` per
   versjon (§4), brukes den tidligste regnmed har lagret for pålegget.
   Advarselen navngir det andre, så selskapet kan kontakte
   innkrevingsmyndigheten.
4. **Trekk i lønnskjøringen.** For hver ansatt finner kjøringen det
   aktive pålegget og perioden som dekker utbetalingsdatoen, og beregner
   trekket. En prosent regnes av det trekkpliktige bruttobeløpet i
   kjøringen — lønn, feriepenger og trekkpliktig del av
   kjøregodtgjørelse — og aldri av trekkfri kjøregodtgjørelse, diett
   eller naturalytelser. Et beløp trekkes som det står. `trekkbeloep` er
   kroner og `trekkprosent` en prosent, begge som JSON-tall (`double`,
   §1). De leses fra tallets **tekst** — kroner til øre, prosenten til
   basispunkter — slik `regnmed-core::currency::parse_rate` leser
   valutakurser, og aldri gjennom et flyttall. En prosent med flere
   enn to desimaler passer ikke i basispunkter og avskjæres ikke: den
   vises som et avvik og trekkes ikke, som `parse_rate` avviser en kurs
   med flere desimaler enn den rommer. Trekket **rundes ned
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
7. **Rapporter** trekket i a-meldingen: `utleggstrekk` med
   `utleggstrekkSamordnet`, negativt beløp og utbetalingsdatoen, og
   `sumUtleggstrekk`.

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
     `trekkversjon`) den brukte, bokføringskallet sender settet tilbake,
     og serveren nekter hvis settet den selv ville brukt er et annet.
     Feltet er **påkrevd** når et pålegg gjelder for kjøringen: et kall
     uten settet nektes som et kall med feil sett. Lønnskjøringen er et
     offentlig API-endepunkt (docs/integrations.md), og en klient som
     ikke kjenner feltet, skal ikke kunne kjøre forbi vakten.
  3. Kjøringen advarer når siste vellykkede henting er eldre enn tre
     dager (tre tapte daglige hentinger, steg 1), siden den da kan
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

1. Test-scopet `skatteetaten:trekkpaalegg` er tildelt BogenTech og lagt
   på testklienten i Samarbeidsportalen. **Bestilt 2026-11-26, sak
   SSV-6690**, på brukerstøttedesken under Hente data → Søknad om
   tilgang (docs/gov.md), ikke med e-posten dokumentasjonen nevner.
2. regnmeds system i Altinns systemregister (TT02) har fått ressursen
   `ske-informasjon-om-trekkpaalegg` (**gjort 2026-11-26**), og
   testselskapets systembruker er godkjent på nytt med den (**gjort
   2026-11-26**, endringsforespørsel godkjent med TestID — docs/gov.md).
3. Skatteetaten har lagt inn trekkpålegg i testmiljøet for det
   syntetiske selskapet.
4. Testmiljøet har vist det spesifikasjonen ikke sier: om `opprettet`
   gjelder pålegget eller versjonen, at et stoppet pålegg står i
   listen som `avsluttet`, og hvilke feilkoder som faktisk kommer.

## 5. Ferdig når

Et trekkpålegg hentet fra Skatteetatens testmiljø blir trukket i en
lønnskjøring, bokført, betalt fra betalingslisten og rapportert i en
a-melding som Skatteetaten tar imot uten avvik. Samme beviskrav som for
a-meldingen og skattekortet i docs/lonn.md.

## 6. Pålegg registrert for hånd (#339) — bygget

API-et dekker ikke to tilfeller (§2, steg 1): **første måned**, før
selskapet har levert en a-melding gjennom regnmed, og **trekk etter den
gamle ordningen**, som bare kommer som brev. Å trekke dem utenfor
regnmed går ikke, siden a-meldingen lages her. Derfor kan et pålegg
legges inn for hånd fra brevet og deretter trekkes, bokføres, betales og
rapporteres i samme kjøring som de hentede.

### Tre typer, tre måter å betale og rapportere

Skatteetatens veiledning [Utleggstrekk i
a-meldingen](https://www.skatteetaten.no/bedrift-og-organisasjon/arbeidsgiver/a-meldingen/veiledning/utleggstrekk/)
(oppdatert 25.02.2026, lest 2026-11-26) har en tabell over typene. Den
er grunnlaget her:

| Type (fra brevet) | I a-meldingen | Betaling | Prosentgrunnlag |
| --- | --- | --- | --- |
| Samordnet (ny ordning) | `utleggstrekkSamordnet`, i sum å betale | én KID **per ansatt**, fra brevet | brutto uten naturalytelser og utgiftsgodtgjørelser (§1) |
| Skatt og avgift (gammel ordning) | `utleggstrekkSkatt`, i sum å betale | **én samlet betaling med én KID** for alle trukket på samme utbetaling — samme KID som selskapet alltid har brukt | bruttolønn, **samme grunnlag som forskuddstrekket** |
| Øvrige krav (gammel ordning) | **rapporteres ikke** | KID og konto fra brevet | står i brevet |

Samordnet og skatt betales innen første virkedag etter utbetaling (§2
steg 5); for øvrige krav sier tabellen «Se trekkpålegget», så fristen
er et felt fra brevet. For alle tre gjelder «trekke så mye som mulig»
og taket i §3, og ingen av dem har sommer- eller desemberfritak.
Veiledningen unntar også mva-, særavgifts- og tollkrav fra
a-meldingen; de er «øvrige» her.

Forskjellen i prosentgrunnlag betyr ingenting i dag, fordi lønnen i
regnmed ikke har naturalytelser (docs/lonn.md), men koden skal skille
dem, så den blir riktig når naturalytelser kommer.

### Hva som registreres

| Felt | Kontroll |
| --- | --- |
| Ansatt | finnes i selskapets ansattregister |
| Type | én av de tre over |
| Beløp per måned **eller** prosent | kroner, lagret som øre; prosent med høyst to desimaler (§2 steg 4). **Øvrige: bare beløp** — grunnlaget for en prosent står i brevet, og regnmed gjetter det ikke |
| Gjelder fra (og eventuelt til) | gyldige datoer, fra ≤ til |
| KID og betalingsmottaker (organisasjonsnummer, kontonummer) | samordnet og øvrige: fra brevet, kontrollsiffer og MOD11. Skatt: ingen — se betalingen under |
| Betalingsfrist | bare øvrige: fra brevet (dager etter utbetaling) |
| Brevet | påkrevd, PDF eller bilde |

Brevet lagres **med pålegget**, innholdsadressert og hash-sjekket ved
hver lesing (docs/blob-store.md), og kan bare leses med `LONN_LES`. Det
festes aldri til lønnsbilaget: et bilagsvedlegg kan leses av alle som
leser hovedboken, også rollen `les`, og brevet navngir den ansatte og
gjelden (§3, punkt 4 under Godkjenning).

### Lagring og endring

- **Innsettings-bart**, som de hentede versjonene: en registrering har
  en id og en versjon, og en endring eller en stopp er en ny versjon,
  aldri en retting. Hver versjon har hvem og når. Statusen beregnes fra
  siste versjon.
- **Hvem:** `LONN_SKRIV` registrerer, endrer og stopper. Har selskapet
  en aktiv attesteringspolicy (docs/attestering.md), må en annen person
  attestere en ny eller endret registrering før den trekkes. Det er den
  eksisterende ordningen med en ny `target_kind` (i dag bare
  `inbox_document`, migrasjon 0030), ikke en egen. Til den er
  attestert, viser forhåndsvisningen registreringen som «venter på
  attestering» og trekker den ikke.
- **Synlighet og tid** som for hentede pålegg (§3, punkt 4 og 5 under
  Godkjenning): registreringen og brevet bare for `LONN_LES`, den
  ansatte ser sitt eget på lønnsslippen, og en registrering som kommer
  etter månedens kjøring trekkes fra neste.
- **I kjøringen:** forhåndsvisningens sett (§3, punkt 2 under
  Godkjenning) bærer (registrering, versjon) ved siden av (`trekkid`,
  `trekkversjon`), så en endring etter forhåndsvisningen stopper
  kjøringen på samme måte. Kontrollen mot hellisten (§2, steg 1) gjelder
  ikke: et brevpålegg står aldri i den.

### Bokføring og betaling

- **Samordnet og øvrige** som steg 5–6: én linje per pålegg på kontoen
  for utleggstrekk, med betalingsmottakeren fra brevet som leverandør
  og brevets KID i `payable_terms`. Forfall er utbetalingsdatoen for
  samordnet og brevets frist for øvrige.
- **Skatt:** alle skattetrekk på samme utbetaling blir **én** linje med
  selskapets egen KID for utleggstrekk skatt — den samme hvert år,
  laget i Skatteetatens KID-generator — og forfall på utbetalingsdatoen.
  KID-en, kontonummeret og mottakerens organisasjonsnummer, alle fra
  Skatteetatens betalingsinformasjon for skatter og avgifter, er nye
  lønnsinnstillinger ved siden av kontoen for utleggstrekk (§2 steg
  5), daterte og innsettings-bare. Parten finnes eller opprettes etter
  regelen i steg 5. Kjøringen nekter å trekke et skattetrekk uten
  innstillingene, med beskjed om hva som mangler. Linjeteksten navngir
  ingen.

### Når et hentet pålegg kommer

- **Samme KID** (første måned: brevet og API-et beskriver samme
  samordnede pålegg): det hentede avløser det manuelle fra neste
  kjøring. Det utledes, ingen skriver noe; forhåndsvisningen sier fra.
  Bare samordnede registreringer kan avløses slik: et skattetrekk har
  selskapets felles KID, som aldri står i et hentet pålegg.
- **Annen KID** (et gammelt trekk og et nytt samordnet): begge vises som
  et avvik, og kjøringen trekker det hentede — et samordnet trekk
  avslutter de andre (§1). Det manuelle står til noen med `LONN_SKRIV`
  stopper det, så stoppen er en registrert beslutning.

### Avgjørelser

- **Attestering bare med policy** (2026-11-26): en annen person kreves
  bare når selskapet har slått på attestering. Å kreve to personer
  alltid ville gjort det umulig for et enkeltpersonselskap å registrere
  et pålegg. Prisen er at et feilslag (40 000 for 4 000, eller en annen
  ansatts KID) trekkes uten at noen andre har sett det i selskaper uten
  policy. Vernet der er at forhåndsvisningen viser hver registrering med
  beløp, KID og lenke til brevet, så den som kjører lønnen kan
  sammenligne før bokføringen.
- **Gammel ordning følger Skatteetatens tabell** (2026-11-26):
  prosentgrunnlag, rapportering og betaling per type som over. Det var
  et åpent spørsmål om de gamle trekkene fulgte samme grunnlag som de
  samordnede; veiledningen sier at de ikke gjør det.

### Bygget slik (2026-11-27)

Det som ikke står over, og som ble avgjort da koden ble skrevet:

- **Brevet lastes opp først** (`POST …/payroll/garnishments/letters`,
  rå kropp, PDF eller bilde) og registreringen peker på det. En endring
  uten nytt brev beholder forrige versjons brev. Brevet lagres i
  `garnishment_letter` med samme offload-regel som de andre
  dokumentene (docs/blob-store.md).
- **Ett pålegg = `garnishment` + `garnishment_version`** (migrasjon
  0110). Ansatt og type ligger fast på pålegget; alt annet per versjon.
  En stopp er en versjon med `stopped`, og et stoppet pålegg kan ikke
  endres — et nytt brev er et nytt pålegg. Triggere nekter UPDATE,
  DELETE og TRUNCATE.
- **Attestering per versjon** (`target_kind = 'garnishment_version'`).
  Om versjonen krever attestering, avgjøres når den skrives: aktiv
  policy da betyr ja. En policy som slås på senere, stopper ikke
  løpende pålegg. Venter en endring på attestering, trekkes den siste
  attesterte versjonen, og forhåndsvisningen sier det; uten noen
  attestert versjon trekkes ingenting. En stopp krever ingen
  attestering. Den som registrerte, kan ikke attestere selv, og en
  utpekt attestant i policyen gjelder også her. Attestanten trenger
  både `ATTESTERING_UTFOR` og `LONN_LES`.
- **Innstillingene** (`payroll_garnishment_setting`) er én rad med hele
  settet — kontoen, KID, kontonummer og mottaker for skatt — med
  `valid_from`; den nyeste med `valid_from` på eller før
  utbetalingsdatoen gjelder. Velger kjøringen kontoen selv (§2 steg 5),
  skriver den en ny rad med kontoen og resten uendret.
- **Rekkefølgen under taket**: samordnet, skatt, øvrige, og innen hver
  type eldste registrering først.
- **En ny mottaker** opprettes som leverandør med navnet
  «Innkrevingsmyndigheten (org.nr. …)»; regnmed slår ikke opp navnet.
- **Settet** sammenlignes to ganger: med det klienten sendte, og på nytt
  inne i kjøringens transaksjon under selskapslåsen. Registrering,
  endring og stopp tar samme lås, så det som bokføres er det som ble
  sjekket.
- **Synlighet i kjøringen**: forhåndsvisningen viser KID, beløp og
  lenke til brevet bare for den som har `LONN_LES`; settet (id-er og
  versjoner) går til alle som kan kjøre lønn, siden det trengs for å
  bokføre. Advarslene navngir den ansatte for alle med `LONN_KJOR`.
- **Prosentgrunnlaget for skatt** er bruttolønnen med feriepenger og
  trekkpliktig godtgjørelse, som for samordnet, pluss naturalytelser
  (null i dag). Om feriepenger skal være med i grunnlaget for den gamle
  ordningen, sier ikke tabellen; testinnsendingen må bekrefte det.
- Dokumentasjonens eksempel på `betalingsmottaker` (971648199) består
  ikke MOD11; registreringen krever gyldig organisasjonsnummer, og
  testene bruker Skatteetatens eget (974761076).

### Utsatt

- **«Samme KID: det hentede avløser det manuelle»** (avsnittet over)
  hører til #334 og er ikke bygget: det finnes ingen hentede pålegg å
  avløse med. Når #334 bygges, skal regelen ligge der settet regnes ut
  (`regnmed-db::garnishment::effective`).
- **Varsel til dem med `LONN_KJOR`** (§3) gjelder hentede pålegg, som
  kommer uten at noen i selskapet har gjort noe. Et pålegg registrert
  for hånd har en person bak, så det varsles ikke.
- **Advarselen om gammel henting** (§3 punkt 3) og **avviket for
  pålegg uten ansatt** (§2 steg 3) gjelder bare hentede pålegg.
- **Å lese `trekkbeloep`/`trekkprosent` fra JSON-tallets tekst** (§2
  steg 4) hører til #334. Portalen leser beløp og prosent som tekst og
  sender heltall (øre og basispunkter), så API-et for registrering tar
  aldri imot et flyttall.

### Ferdig når

Et pålegg registrert for hånd i testmiljøet blir trukket, bokført,
betalt fra betalingslisten og rapportert i en a-melding som
Skatteetaten tar imot uten avvik, og et hentet pålegg med samme KID
avløser det i neste kjøring. Et skattetrekk etter gammel ordning
rapporteres som `utleggstrekkSkatt` og betales som én samlet post.
