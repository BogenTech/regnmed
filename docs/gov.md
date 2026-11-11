# The government rail: Maskinporten and Skatteetaten's APIs

Everything regnmed sends to Norwegian authorities rides on
**Maskinporten** (Digdir's machine-to-machine OAuth2 server). The client
lives in `crates/regnmed-gov` and is shared by every government
integration — mva-melding first, later skattemelding and årsregnskap.

## Maskinporten (crates/regnmed-gov/src/maskinporten.rs)

Flow (RFC 7523): sign a short-lived JWT grant with the key registered on
our Maskinporten client → POST to the token endpoint → receive an access
token carrying the requested scopes → cache until shortly before expiry.
Grant constraints honored: `exp − iat ≤ 120 s`, unique `jti`, `aud` =
the Maskinporten issuer.

Configuration (environment):

| Variable | Example |
| --- | --- |
| `MASKINPORTEN_TOKEN_ENDPOINT` | `https://test.maskinporten.no/token` |
| `MASKINPORTEN_AUDIENCE` | `https://test.maskinporten.no/` |
| `MASKINPORTEN_CLIENT_ID` | client UUID from Samarbeidsportalen |
| `MASKINPORTEN_KEY_FILE` | path to the RS256 private key (PEM) |
| `MASKINPORTEN_KID` | key id, if several keys are registered |
| `MASKINPORTEN_SCOPES` | `skatteetaten:mvameldingvalidering` |

Production uses `https://maskinporten.no`. **No secrets in the repo,
ever** — keys live outside git and outside container images. That rule
is absolute, in test as well as production — there is no encrypted-secrets
carve-out. Where the keys actually live is recorded in docs/secrets.md.

### Operational setup (one-time, per environment)

**Status: TILDELT 2026-08-10** — svar på sak SSV-5760: BogenTech AS
(935115086) har fått følgende tilganger i **test**: Innrapportering
A-melding (DM-1), Innrapportering aksjonærregisteroppgave (DM-8), MVA
Melding Validering (DM-39), Skattekort til arbeidsgiver (DM-41).
Bestillingen ble sendt 2026-08-06 på brukerstøttedesken
(eksternjira.sits.no, kategori Innrapportering), tjenesteområder
A-meldingen, Aksjonærregisteret, MVA-melding og Skattekort til
arbeidsgiver, akseptert bruksvilkår. Skjemaet spør aldri om klient-id —
tilgangen gis organisasjonen, og scopene legges på klienten i
Samarbeidsportalen etterpå. Historikken bak: den
opprinnelige bestillingen 2026-07-24 gikk via skriv-til-oss, som
2026-08-06 svarte at kanalen ikke behandler slike saker; omveien
kostet lite, siden juli-bestillingen ba om
`skatteetaten:mvameldinginnsending` som ikke finnes (innsending går
via Altinn 3, se tabellen under).

Veien inn, for neste gang (dette var det som stanset bestillingen):
**det finnes ikke noe registreringsskjema** — daglig leder/styreleder
har automatisk tilgang til brukeradministrasjonsverktøyet
(<https://skatt.skatteetaten.no/web/sakservice-web/>), oppretter
brukerkontoene der (gjort 2026-08-06, andre@bogentech.no), og første
pålogging på desken går via «Har du glemt passordet ditt?». Skal
andre enn daglig leder administrere: deleger Altinn-tjenesten
«Brukeradministrasjon – brukerstøtte for bruk av Skatteetatens
opplysninger» først.

Gjenstår:

1. ~~Legg de fire tildelte scopene på testklienten~~ **GJORT
   2026-08-10** (alle fire GRANTED, se under). Legg de fire tildelte scopene på testklienten i Samarbeidsportalen
   (tildelingen gjelder organisasjonen; klienten må fortsatt bære
   scopene), registrer denne maskinens nye nøkkel (se nøkkelnotatet
   under), og kjør valideringsrundturen (steg 4 under) — empirisk
   sjekk: `scripts/maskinporten-scope-test.py <scope>` skal svare
   GRANTED for alle fire.
2. `skatteetaten:mvaregisteravgiftssubjekt` og `skatteetaten:frister`
   var **ikke** valgbare i Innrapportering-skjemaet — de hører til
   Deling-siden (eget behandlingsgrunnlagskrav) og bestilles separat
   der når de trengs. Lav prioritet: begge er kryssjekker, ikke
   forutsetninger.
3. ~~Altinn 3-tilgangen for mva-melding-INNSENDING~~ **BESVART
   2026-08-11**: `altinn:instances.read`/`write` bestilles i Digdirs
   registreringsskjema for sluttbrukersystem, ikke hos Skatteetaten.
   Den opprinnelige noten sto slik, og var riktig om skjemaet vi da
   så på:
   (`altinn:instances.read`/`write` mot mva-melding-appen) hadde ikke
   noe felt i skjemaet — still spørsmålet som oppfølging på SSV-5760
   når saken får en behandler.
4. Produksjon bestilles i samme skjema når testintegrasjonen er
   verifisert.

The **test client exists** with an RS256 keypair registered on it; its
id and the rest of the `MASKINPORTEN_*` configuration live outside git
in `~/.config/regnmed/maskinporten-test.env` (docs/secrets.md).
**Nøkkelnotat 2026-08-10:** denne utviklingsmaskinen hadde ingen
`~/.config/regnmed/`, så en ny nøkkel ble generert etter
per-maskin-oppskriften (`scripts/maskinporten-key.sh`) og registrert
på klienten samme dag (kid = RFC 7638-thumbprinten — portalen beholdt
JWK-ens egen kid denne gangen, utløper 10.08.2027). Den gamle nøkkelen
(kid `6e9b86f2-…`, utløper 24.07.2027) er fortsatt I BRUK på den andre
MacBook-en — to maskiner, to nøkler, akkurat slik docs/secrets.md
foreskriver.

The aksjonærregisteroppgave scope rides in SSV-5760 — and additionally
needs an Altinn systembruker, see below.

1. Get access to Digdir's **Samarbeidsportalen** (requires the
   organization's Altinn roles).
2. Create a Maskinporten client in the test environment; register a
   public key (or virksomhetssertifikat) on it.
3. Request the Skatteetaten scopes. **Verified against Skatteetatens
   own API-dokumentasjon 2026-07-27** — the earlier note here was partly
   wrong, so check the API's own page before ordering:

   | Trenger | Scope |
   | --- | --- |
   | Mva-melding, validering | `skatteetaten:mvameldingvalidering` |
   | Mva-melding, **innsending** | ingen `skatteetaten:`-scope — går via Altinn3 med `altinn:instances.read` / `altinn:instances.write` |
   | A-melding (#46) | `skatteetaten:innrapporteringamelding` |
   | Skattekort (#46) | `skatteetaten:skattekorttilarbeidsgiver` |
   | Aksjonærregisteroppgaven (#43) | `skatteetaten:innrapporteringaksjonaerregisteroppgave` |

   Merk at `skatteetaten:mvamelding` er et ANNET API — det *leverer
   fastsatte* mva-meldinger (leser inn), ikke innsending. Navnelikheten
   er en felle.

   **Alle Skatteetaten-scopene bestilles ett sted.** Verifisert
   2026-07-28: hver SBS-side («mva-melding», «a-meldingen»,
   «aksjonarregisteroppgaven») lenker under «trenger du hjelp» til den
   SAMME brukerstøttetjenesten, med samme setning — *«Send oss en
   henvendelse her hvis du har spørsmål knyttet til en tjeneste, skal
   bestille tilgang til en ny tjeneste eller vil melde inn
   endringsønsker»*:

   <https://eksternjira.sits.no/plugins/servlet/desk/site/global>

   Én henvendelse kan altså be om alle scopene samtidig. En tidligere
   utgave av dette dokumentet listet én rad per scope med lenke til
   API-ets egen side; det var å notere hvilken side man går *fra*, ikke
   hvor den fører — og fikk bestillingen til å se ut som fire søknader.

   Tjenesten krever egen brukerkonto (ikke ID-porten): virksomheten må
   først opprette en brukeradministrator som lager kontoene. Førstegangs
   pålogging går via «Har du glemt passordet ditt?».

   Oppgi alltid **organisasjonsnummer**, **miljø** (test/prod) og
   **klient-id**, og bestill test og produksjon hver for seg.

### Hele scope-behovet, ikke bare det neste

Kartlagt 2026-07-27 ved å gå gjennom Skatteetatens API-dokumentasjon i
sin helhet (99 API-sider), fordi å bestille ett scope om gangen er den
dyreste måten å gjøre dette på: hver tildeling har ukers ledetid.

**Bestilles nå — dekker kode som finnes eller er neste steg:**

| Scope | Til | Status |
| --- | --- | --- |
| `skatteetaten:mvameldingvalidering` | mva-melding, validering (#8) | koden finnes |
| `skatteetaten:innrapporteringamelding` | a-melding **og** avstemmingsrapporten (#46) | koden mangler |
| `skatteetaten:skattekorttilarbeidsgiver` | skattekort → forskuddstrekk (#46) | koden mangler |
| `skatteetaten:innrapporteringaksjonaerregisteroppgave` | RF-1086 (#43) | rendringen finnes |
| `skatteetaten:mvaregisteravgiftssubjekt` | er selskapet mva-registrert? | i dag et manuelt flagg i firmaopplysningene (docs/faktura.md) — dette er den autoritative kilden |
| `skatteetaten:frister` | offisielle frister | vi *beregner* mva-frister selv (`Terminordning`); en autoritativ kilde er en kryssjekk, ikke en erstatning |

**Vent til saken er planlagt** — ikke bestill tilgang til opplysninger
uten et sted å bruke dem; det er både dataminimering og noe
Skatteetaten spør om:

| Scope | Til |
| --- | --- |
| `skatteetaten:skattemeldingupersonlig`, `skatteetaten:naeringsspesifikasjon` | skattemelding m/ næringsspesifikasjon (#11, «later») |
| `skatteetaten:formueinntekt/skattemelding` | innsending av skattemelding (#11) |

⚠️ **Men innsending av skattemelding går ikke på denne skinnen i det
hele tatt.** Skatteetaten skriver selv, om overgangen til Altinn 3:
*«Validering og innsending må fortsatt gjøres med ID-porten»*. For
inntektsår 2025 gir Maskinporten bare **lesetilgang** — hente gjeldende
skattemelding og PDF av den fastsatte. Å bestille scopene over gir
altså ikke en innsendingsvei; den krever en innlogget person.
Konsekvensene for #11 står i docs/skattemelding.md.

**Ikke aktuelt for regnmed:** primærnæringsscopene (med mindre et
kundesegment krever det), oppdragsregisteret (bygg/anlegg),
innkrevings- og utleggsscopene (inkasso overlates bevillingshavere,
docs/purring.md), og tredjepartsrapporteringen for bank/forsikring.

### Det som IKKE er Skatteetaten-scope

Den fellen som kostet en runde: to av behovene løses ikke med et
`skatteetaten:`-scope i det hele tatt, og må søkes hos Digdir/Altinn
**parallelt**, ikke etterpå.

| Behov | Krever |
| --- | --- |
| Mva-melding, **innsending** | Altinn3-instansflyten med `altinn:instances.read` / `altinn:instances.write` |
| Aksjonærregisteroppgaven | **Altinn systembruker** med tilgangspakke, i tillegg til scopet |

`altinn:instances.read` / `altinn:instances.write` er synlige i
Samarbeidsportalens scope-velger og kan hukes av — men det gir ingen
tilgang, akkurat som for Skatteetaten-scopene (verifisert 2026-07-28:
klienten fikk dem lagt til, og portalen svarte «Det er lagt til scopes
på klienten som virksomheten din ikke har tilgang til»).

**LØST 2026-08-11.** Skatteetaten bekreftet på SSV-5760 at innsending
krever nettopp `altinn:instances.write` / `.read`, og bestillingsveien
er Digdirs **registreringsskjema for sluttbrukersystem** (steg 1.1 i
deres onboarding) — funksjonsområdet «Innsending av skjema/data» ER
`altinn:instances.write`. Altså samme skjema som systembruker-scopene,
ikke en egen kanal. Avsnittet under sto som åpent spørsmål til da:

Altinns egen dokumentasjon beskriver bare hvordan **tjenesteeiere** får
disse scopene delegert, ikke hvordan et sluttbrukersystem får dem. Vi
hadde altså **ingen verifisert bestillingsvei** for dem — ikke skriv
en inn her før den er prøvd. Det praktiske første forsøket er å spørre i
samme henvendelse til Skatteetatens brukerstøtte: innsendingsflyten er
deres, så de vet hvilken delegering den krever.

4. Point the env variables above at the test environment and run
   `regnmed mva-melding … --validate`.

For a regnskapsfører acting on behalf of clients, the client company
**delegates** the scope to the accounting firm in Altinn (Altinn
Autorisasjon). This maps 1:1 onto regnmed's engagement model — the same
firm→client relationship, expressed in the government's registry.

## A-melding API (crates/regnmed-gov/src/amelding.rs)

`AmeldingClient` sender en a-melding og henter tilbakemeldingen, begge
med kundens **systembrukertoken**
(`TokenProvider::token_for_systemuser`, scopene i `amelding::SCOPES`:
`skatteetaten:innrapporteringamelding digdir:dialogporten`). Kroppen er
`regnmed_core::amelding::render_json` (docs/lonn.md).

- `submit` → `POST {AMELDING_URL}/v1/innsending/{periode}/{opplysningspliktig}`
  med `idempotencyKey`; svaret er en kvittering (`dialogId`,
  `forsendelseId`, `meldingsId`), ikke en dom. Feil fra Skatteetaten
  (`kode`, `melding`, `spesifisering`) sendes videre ordrett.
- `feedback` → dialogen i Dialogporten (`DIALOGPORTEN_URL`), den
  `Information`-transmissionen som er relatert til innsendingen, og
  dens `Api`-URL. `None` til den finnes.
- `from_env` peker på TESTMILJØET når variablene mangler — produksjon
  velges uttrykkelig, aldri ved et uhell.

**Prøvd mot ekte testmiljø 2026-11-11**:
`crates/regnmed-gov/tests/amelding_live.rs` rendrer en melding med
regnmeds egen kode, sender den med klienten og venter på dommen —
`mottatt`, `avvik: []`, etter 24 s. Testen hopper over seg selv uten
`AMELDING_LIVE=1` og kjører aldri i CI; hver kjøring er en ekte
innsending i test (og legges til periodens summer — se docs/lonn.md om
hvorfor en ny melding for samme måned ADDERER).

Koblingen til lagrede lønnskjøringer — innsendingssporet med
idempotensnøkkel, meldingsId og erstatningsmelding, API-endepunktene og
portalknappene — står i docs/lonn.md («Innsendingssporet»), og
tilbakemeldingen som Altinn-hendelse (`regnmed-gov::altinn_events`,
webhook `/webhooks/altinn`) i «Tilbakemelding som hendelse». Merk:
Events-API-et krever et VEKSLET Altinn-token, ikke Maskinporten-tokenet.

## Mva-melding APIs (crates/regnmed-gov/src/mvamelding.rs)

- **Validation** (implemented): POST the melding XML to Skatteetaten's
  validation endpoint (test:
  `https://idporten-api-test.sits.no/api/mva/grensesnittstoette/mva-melding/valider`,
  override with `MVA_VALIDATION_URL`). The returned
  `valideringsresultat` is kept verbatim — it is documentation of a
  control — and any avvik fail the CLI run.
- **Submission** (pending): the Altinn3 instance flow (create instance →
  upload melding + konvolutt → confirm → poll feedback). Deliberately
  not implemented until we hold real test credentials, so it can be
  developed against the actual test environment instead of guessed.
  Tracked in issue #8.

## Aksjonærregisteroppgaven (crates/regnmed-core::aksjonaeroppgave)

RF-1086 rides the same rail, and is the most urgent of them:
**from June 2026 an end-user system is the only way to file** — Altinn.no
and paper are gone (docs/aksjonaer.md).

- **Rendering** (implemented): hovedskjema + one underskjema per
  shareholder, validated against Skatteetatens official XSDs (vendored
  in `docs/aksjonaer/`) in unit tests, the integration test and CI.
- **Submission** (pending, same posture as the mva-melding): scope
  `skatteetaten:innrapporteringaksjonaerregisteroppgave` must be ordered
  from Skatteetaten, and the API additionally requires an **Altinn
  systembruker** with an access package
  (`regnskapsforer-med-signeringsrettighet`, `ansvarlig-revisor`, …) —
  a newer Digdir mechanism we have not set up. The flow is POST
  hovedskjema → POST underskjema (×N) → POST bekreft, each call carrying
  a unique `idempotencyKey`.

Not implemented until we can run it against the real test environment,
for the same reason as #8: a client written against an API we cannot
execute is guesswork in another layer.

## Er scopet faktisk tilgjengelig? Spør Maskinporten

Samarbeidsportalen viser en **katalog** over alle registrerte scope —
ikke en liste over hva virksomheten har fått tildelt. At et scope er
synlig betyr altså ikke at det kan brukes. (Katalogen inneholder blant
annet Skatteetatens egne skrape-scope som `skatteetaten:frodetest657`
«frode tester create» og `skatteetaten:kjetiltest` «kun for test» —
ingen har delegert dem til noen.)

`scripts/maskinporten-scope-test.py <scope>` gjør spørsmålet empirisk,
og skiller de tre tilstandene som ellers blandes sammen:

| Svar | Betyr |
| --- | --- |
| `GRANTED` | virker |
| `IKKE TILDELT` | «Consumer has not been granted access» — Skatteetaten har ikke gitt virksomheten tilgang. **Må bestilles.** |
| `IKKE PÅ KLIENTEN` | «invalid scopes for client» — scopet er ukjent for klienten |
| `OPPSETTFEIL` | `invalid_grant`: nøkkel, kid eller klient-id |
| `INGEN SYSTEMBRUKER` | MP-303, bare med `--systemuser <orgnr>`: grantet er gyldig, men kunden har ikke godkjent noen systembruker for systemet vårt i Altinn |

**Å huke av et scope i Samarbeidsportalen gir ingen tilgang.** Det er
den viktigste lærdommen her, og den er testet: da a-melding-scopet ble
lagt til på klienten, endret svaret seg fra «invalid scopes for client»
til «Consumer has not been granted access» — altså fra én blokkering til
en annen. Katalogen viser hva som *finnes*, tildelingen avgjør hva som
kan *brukes*, og bare Skatteetaten kan gjøre det siste.

**Status 2026-07-27:** klient og nøkkel autentiserer (kid `6e9b86f2-…`,
satt i `~/.config/regnmed/maskinporten-test.env`) — hele JWT-grant-kjeden
virker. `skatteetaten:innrapporteringamelding` og
`skatteetaten:skattekorttilarbeidsgiver` svarer **IKKE TILDELT**, altså
ligger bestillingen hos Skatteetaten. Bestill med de verifiserte
navnene i tabellen over, ikke fra hukommelsen.

**Status 2026-08-10:** tildelingen fra Skatteetaten er bekreftet på
SSV-5760 (DM-1, DM-8, DM-39, DM-41 — se øverst), og **empirisk
verifisert samme dag**: alle fire scopene svarer **GRANTED** med den
nye maskinnøkkelen. Hele kjeden nøkkel → grant-JWT → token virker.

### Valideringsrundturen: mva-melding vil ha ID-PORTEN, ikke Maskinporten

Dette er dagens viktigste funn (2026-08-10), og det flytter #8.
Meldings-XML-en bygges fint fra demo-boken, men rundturen fullfører
ikke — av to grunner som ble blandet sammen i første diagnose:

**1. Vi leste feil dokumentasjonssted, og URL-en var utdatert.**
`idporten-api-sbstest.sits.no` — verten på mva-meldingens eget
dokumentasjonsnettsted — fullfører TLS-håndtrykket (gyldig
Sectigo-sertifikat) og RESETTER så ALLE forespørsler: hver sti, også
`/` og port 80, med og uten token, TLS 1.2 og 1.3, fra tre uavhengige
nett (hjemme bak Check Point-VPN, homelab, amerikansk sky). Verten er
åpenbart avviklet. Den riktige er `idporten-api-**test**.sits.no`
(uten `sbs`), som svarer 401 som seg hør og bør, og den står i
**api-dokumentasjon-katalogen** — `Skatteetaten/api-dokumentasjon`,
`docs/api/*.md`. **Den katalogen er fasiten**, ikke de eldre
per-tjeneste-nettstedene: den har ett oppslag per API med scope,
token-type, testvert og lenke til OpenAPI-spesifikasjonen på
SwaggerHub. Vi brukte flere timer på et feilsøk som ett oppslag der
ville avsluttet.

**2. Og da svarer API-et hva som faktisk er galt:**

```json
{"feilkode":"IDPORTEN_TOKEN_FEIL_FORMAT","feilmelding":"Ingen tilgang",
 "tekniskeDetaljer":"403 - Forbidden"}
```

Valideringstjenesten vil ha et **ID-porten-token**, ikke vårt
Maskinporten-token. Skatteetatens egen dokumentasjon sier det samme,
lest på nytt etterpå: scopene for validering og innsending listes
sammen med **`openid`** — som bare en brukerpålogging bærer — og om
Maskinporten står det gjennomgående *«det kommer senere»*. Maskinporten
er dokumentert for **innsynstjenestene** (`mp-test.sits.no`,
`.../innsyn/...`), altså det som LESER fastsatte meldinger.

**Konsekvens: #8s gjenstående halvdel er nedstrøms #26**
(ID-porten-føderering via regnid) — nøyaktig samme funn som
docs/skattemelding.md alt hadde gjort for #11, men ROADMAP-en fanget
det ikke for mva-meldingen. Selve klienten er ikke bortkastet: formen
på kallet er riktig, det er token-KILDEN som må byttes.

### Hva hver av de fire tildelingene faktisk krever

Prøvd med ekte token mot testmiljøet 2026-08-10 — ikke lest, kjørt.
Svarene skiller seg fra hverandre på en måte som er verdt å merke seg:
**401 betyr at selve token-TYPEN ble avvist, 403 at den ble godtatt og
noe annet mangler.**

| Tildeling | Skinne | Svar i dag | Mangler |
| --- | --- | --- | --- |
| DM-39 mva-validering | **ID-porten** | 401 | brukerpålogging (#26) |
| DM-1 a-melding | Maskinporten | 403 `AMLD_1063` | systembruker |
| DM-41 skattekort | Maskinporten | 403 | systembruker |
| DM-8 aksjonærregister | Maskinporten | (dok.) | systembruker |

A-meldingen sier det rett ut, og er derfor kilden til hele
konklusjonen: *«Tokenet mangler informasjon om systembruker i
`authorization_details`»*.

### Skatteetatens svar (SSV-5760, 2026-08-11): det er loven, ikke etterslep

Vi spurte, og svaret er entydig — og viktigere enn selve blokkeringen:

> «Støtte for Maskinporten er IKKE på veikartet pga lovregulering.
> MVA-loven krever å vite hvem (fnr) som har sendt inn melding (noe
> maskinporten ikke støtter — maskinporten oppgir kun orgnr)»

**Dette er altså ikke et hull som tettes senere.** Den eldre
dokumentasjonen sa «vi ser på Maskinporten, det kommer» — det er
foreldet, og premisset er nå navngitt: mva-meldingen må kunne
tilskrives et **menneske**, og Maskinporten bærer per konstruksjon bare
et organisasjonsnummer. Ingen tidsplan kan endre det; det ville krevd
en lovendring.

Skatteetaten bekreftet samtidig de tre andre punktene: at vi leste
utgått GitHub-dokumentasjon (fasiten er
`skatteetaten.github.io/api-dokumentasjon`), at valideringsverten er
`idporten-api-test.sits.no` — nøyaktig den vi selv fant og rettet til
2026-08-10 — og at innsending krever `altinn:instances.write`/`.read`.

**Konsekvensen er en ren linje gjennom hele regjeringsskinnen**, og den
er verdt å holde fast ved når nye API-er dukker opp:

| Familie | Tilskrives | Skinne | Kan automatiseres? |
| --- | --- | --- | --- |
| **Fastsetting** — mva-melding, skattemelding | et **menneske** (fnr) | ID-porten | **Nei.** En innlogget person, hver gang |
| **Innrapportering** — a-melding, skattekort, RF-1086 | en **virksomhet** (orgnr) | Maskinporten + systembruker | Ja |

Den linjen faller heldigvis sammen med hvordan regnmed alt er bygget:
utsendelse, betaling og attestering er allerede uttrykkelige
menneskehandlinger, aldri CronJobs. En mva-melding som sendes inn av
seg selv klokka 03:00 ville vært feil design uansett hva loven sa —
her sier loven det samme.

**#8 er derfor permanent nedstrøms #26**, ikke midlertidig. Det samme
gjelder #11 (docs/skattemelding.md). Valideringsklienten er ikke
bortkastet: formen på kallet er bekreftet riktig, og den venter bare på
en token-kilde vi skal bygge uansett.

**Altså: ÉN ting låser opp tre av fire — Altinn systembruker.** Det er
Digdirs nyere mekanisme, den samme docs/aksjonaer.md alt hadde notert
for RF-1086, og den gjelder hele den reetablerte Altinn-generasjonen av
Skatteetatens API-er. Oppskriften står i api-dokumentasjonens
`docs/om/systembruker.md`: bestill Digdir-scopene
(`altinn:authentication/systemregister.write`,
`…/systemuser.request.read`, `…/systemuser.request.write`,
`digdir:dialogporten`) via Digdirs skjema, registrer systemet i
systemregisteret knyttet til vår klient-id, og oppgi tilgangspakkene
API-ene krever. Det er nå det høyest prioriterte neste steget på denne
skinnen — ikke fordi det haster mest, men fordi det låser opp mest.

**Bestilt 2026-11-11** (sporet i #180): Digdirs registreringsskjema
«Sluttbrukersystem i Altinn 3» sendt for **test**, med
funksjonsområdene Innsending av skjema/data, Overvåke/hente fra innboks,
Systembruker og Klientdelegering. Samme dag signerte BogenTech
**bruksvilkårene for sluttbrukersystemleverandører i Altinn** i
Samarbeidsportalen. Vilkårene kreves først før produksjon (§1.1), men
§1.2 legger et krav på oss før vi går dit: kunden er fullt ansvarlig
for det vi gjør i Altinn på deres vegne, så hver kunde må ha en avtale
med oss som viderefører vilkårene. Utkastet står i docs/kundevilkar.md
(ikke gjeldende før juridisk gjennomgang og aksept i portalen).

**Token-siden er klar før tildelingen** (2026-11-11):
`TokenProvider::token_for_systemuser(orgnr)` i
`crates/regnmed-gov/src/maskinporten.rs` legger Digdirs RAR-utvidelse
i grantet — `authorization_details` med `type: urn:altinn:systemuser`
og kundens orgnr som `0192:<orgnr>` (docs.digdir.no, «Systembruker for
virksomheter») — og cacher ett token per kunde, adskilt fra vårt eget.
Formen er festet i enhetstest. Prøvd mot ekte test-Maskinporten samme
dag med `scripts/maskinporten-scope-test.py <scope> --systemuser
935115086`: svaret er **MP-303** `invalid_altinn_customer_configuration`
(«No access or invalid configuration of the requested system-user»),
mens samme scope uten systembruker svarer GRANTED. Grantet er altså
godtatt; det som mangler er systemet i systemregisteret og en godkjent
systembruker — nøyaktig det bestillingen over låser opp. `externalRef`
er bevisst utelatt (én systembruker per kunde per system).

Den fjerde, mva-valideringen, låses ikke opp av systembruker i det hele
tatt — den er fastsetting, og venter på #26 for godt (se over).

**Lærdommen, to hakk dypere enn den forrige i dette dokumentet:**
katalogen viser hva som *finnes*, tildelingen avgjør hva som kan
*utstedes*, **API-et avgjør hvilken token-TYPE det godtar** — og selv
riktig type kan mangle **systembruker**. At DM-39 ble tildelt og at
scope-testen svarer GRANTED beviser bare de to første leddene. Bare et
ekte kall beviser resten.

### Systembruker: hvilke tilgangspakker, og hvorfor det er to systemer

Kartlagt 2026-08-10 mot Altinns egen dokumentasjon og deres levende
tilgangspakke-API
(`platform.tt02.altinn.no/accessmanagement/api/v1/meta/info/accesspackages/export`).
Skatteetatens advarsel om å «ikke kombinere tilgangspakker på tvers av
fullmaktsområder» er et symptom; **den egentlige regelen er strengere og
står hos Altinn**.

**Regel 1 — klientpakker er OG, ikke ELLER.** Altinn skriver at
tilgangspakker for klientforhold *«fungerer kun for AND-tilfeller»*: har
systembrukeren flere pakker, må klienten ha **alle** for at den skal
virke i det hele tatt. Å bunte pakker gjør altså systemet mindre
brukbart, ikke mer. Derfor er regnskapsfører- og revisorpakker
utelukket fra samme system — et selskaps regnskapsfører og revisor er
to forskjellige foretak, så ingen klient vil noensinne ha begge.

**Regel 2 — systemregisteret godtar bare klientpakkene.** Feilkoden
`AUTH.VLD-00008` sier at pakker må være del av **REGN/REVI/
Forretningsfører**. `a-ordning`, `lonn`, `skattegrunnlag` og
`skatt-naering` — som API-ene ellers godtar — kan altså **ikke**
registreres som `accessPackages`. De hører til fullmaktsområdene
Personale og «Skatt, avgift, regnskap og toll», som en virksomhet
delegerer direkte.

Det er nettopp derfor regnmed trenger **to systemregistreringer**, én
per kundeform vi allerede har i produktet (#77s to ruter):

| Rute | Mekanisme | Registreres med |
| --- | --- | --- |
| «Egen virksomhet» — selskapet rapporterer selv | standard systembruker | `rights` med Skatteetatens ressurser |
| «Regnskapsfører» — byrået rapporterer for klientene | klient-systembruker | `accessPackages` fra REGN |

Ressursene til den første (fra API-dokumentasjonen, ikke gjettet):
`ske-innrapportering-amelding`, `ske-skattekort-til-arbeidsgiver`,
`ske-innrapportering-aksjonaerregisteroppgave`. Den ruten berøres ikke
av regel 1 i det hele tatt.

Til den andre gir ER-rollen «Regnskapsfører» disse tre pakkene, og
API-ene godtar dem slik:

| Pakke | a-melding | skattekort | aksjonærreg. |
| --- | --- | --- | --- |
| `regnskapsforer-med-signeringsrettighet` | ✓ | ✓ | ✓ |
| `regnskapsforer-uten-signeringsrettighet` | ✓ | ✓ | ✓ |
| `regnskapsforer-lonn` | ✓ | ✓ | — |

**Anbefaling (vår slutning, ikke Altinns instruks): én pakke per
klient-system.** Med OG-semantikken er et system med `med-` OG `uten-
signeringsrettighet` ubrukelig for begge slags byråer, mens to systemer
med én pakke hver virker for hver sin. De to signeringsvariantene
dekker alle tre API-ene alene, så det er nok å begynne med.

**Revisor er bevisst utelatt.** regnmeds revisoroppdrag er lesetilgang
+ kjedeverifikasjon (CLAUDE.md), og alle tre API-ene er
rapportering. Skulle det endres, blir det et eget system i
revisorområdet — aldri blandet inn i regnskapsførersystemet.

### Registrert 2026-11-11: ÉTT system, ikke to

To funn under selve registreringen i TT02 endret planen over, og
begge er Altinns egne regler, ikke vår slutning:

1. **Én Maskinporten-klient kan bare knyttes til ett system**
   (`AUTH.VLD-00004`). Systembrukertokenet finner systemet gjennom
   klient-id-en, så to systemer ville krevd to klienter med hver sin
   nøkkel og hver sin `TokenProvider`.
2. **Forespørselen velger pakkene, ikke systemet.** En
   klient-systembrukerforespørsel (`…/systemuser/request/vendor/agent`)
   har sin egen `accesspackages`-liste. OG-regelen (regel 1) gjelder
   altså pakkene på SYSTEMBRUKEREN — og den får én pakke, den
   forespørselen ber om. Anbefalingen «én pakke per system» over løser
   derfor et problem som ikke oppstår når forespørselen alltid ber om
   én.

Derfor er regnmed registrert som **ett system, `935115086_regnmed`**,
med begge kundeformene: `rights` med de tre Skatteetaten-ressursene
(egen virksomhet) og `accessPackages` med de tre REGN-pakkene
(regnskapsfører). Hver forespørsel ber om sin del: en standard
systembruker om rettighetene, en klient-systembruker om ÉN pakke.
Registreringen godtok ressursene (`AUTH.VLD-00003` ville sagt fra om
noen ikke fantes).

**`isVisible` må være `false`** når systemet har klientpakker
(`AUTH.VLD-00018`). Systemet listes da ikke i Altinn for en kunde å
velge selv — hver systembruker starter som en forespørsel regnmed
sender, og kunden godkjenner den via `confirmUrl`. Det var planen
uansett: tilgangen skal begynne i regnmed, der kunden alt er.

Registreringen er kode, ikke klikk: `scripts/altinn-system.py`
(`show`/`register`/`update`) holder hele definisjonen, så produksjon
registreres med samme innhold når testen er verifisert. Etter
registreringen svarer `scripts/maskinporten-scope-test.py <scope>
--systemuser 935115086` fortsatt **MP-303** — riktig, ingen kunde har
godkjent en systembruker ennå.

**Første systembruker, 2026-11-11.** Forespørsel
(`scripts/altinn-system.py request <orgnr>`, rettighetene til egen
virksomhet) sendt for det syntetiske Tenor-selskapet KREATIV INNESLUTTET
TIGER AS (311032240) og godkjent av dets daglige leder med TestID i
TT02 (`status <id>` → `Accepted`). Deretter utstedte test-Maskinporten
systembrukertoken for alle tre scopene (a-melding, skattekort,
aksjonærregister) med `--systemuser 311032240`, og tokenet bærer
`authorization_details` med `systemuser_id` og
`system_id: 935115086_regnmed` — samme form som Skatteetatens eget
eksempel. Hele kjeden fra registrering til token er dermed prøvd mot
ekte testmiljø; neste ledd er et ekte kall mot et av Skatteetatens
API-er med tokenet.

Tre ting å vite til neste gang:

- **Tenor-listen blander hovedenheter og underenheter.** En
  systembruker gis av den juridiske enheten; underenhetens
  «Relasjoner»-fane peker til hovedenheten, og daglig leder står i
  hovedenhetens kildedata (`rollegrupper`, kode `DAGL`).
- **Godkjenning i TT02 skjer med TestID, ikke BankID.** Test-ID-porten
  tilbyr også BankID, men det er BankIDs testmiljø, som krever
  test-BankID med kodebrikke. Velg «TestID på nivå høyt» og skriv inn
  det syntetiske fødselsnummeret.
- **Godkjenningssiden viste «levert av N/A»** med riktig orgnr — TT02
  kjenner ikke BogenTechs navn. Sjekk at navnet vises i produksjon.

### Første a-melding, 2026-11-11: mottatt uten avvik

Første ekte kall mot Skatteetatens a-melding-API, med
systembrukertokenet over: `POST /v1/innsending/2026-10/311032240` på
`innrapporteringamelding.api.skatteetaten-test.no`
(`innrapportering-amelding-api` 0.1.2, påkrevd header
`idempotencyKey`). Innholdet var en hel månedsmelding for det
syntetiske selskapet — underenhet 314752538, én inntektsmottaker (den
daglige lederen), fastlønn 40 000, forskuddstrekk 14 000, aga 14,1 % =
5 640 — i nøyaktig den formen `regnmed-core::amelding` rendrer, bare
som JSON. Svaret var **200** med `dialogId`, `forsendelseId` og
`meldingsId`.

Tilbakemeldingen kom 21 sekunder senere: `mottakstatus: "mottatt"`,
**`avvik: []`**, summene speilet tilbake og `kildesystem: "regnmed"`.
Den inneholder også kontonummer og KID for innbetaling av
arbeidsgiveravgift, forskuddstrekk og finansskatt — det
betalingslisten (#33) trenger for å betale dem.

Det som er verdt å vite til klienten skal bygges:

- **JSON-en er samme tre som XML-en**, felt for felt med samme navn.
  Gjentatte elementer blir lister (`virksomhet`, `inntektsmottaker`,
  `arbeidsforhold`, `inntekt`, `forskuddstrekk`,
  `loennOgGodtgjoerelse`, `sumForskuddstrekkPerLoennsutbetalingsdato`);
  `BeloepSomHeltall` er JSON-tall, `BeloepSomDesimaltall` og
  `Desimaltall` er strenger, `Boolsk` er `true`/`false`. Renderen
  trenger altså en JSON-serialisering av samme modell, ikke en ny
  modell.
- **Tilbakemeldingen slås IKKE opp med `forsendelseId`.** Den gir 404
  (`AMLD_1050`). Den finnes som en EGEN transmission i dialogen —
  type `Information`, `relatedTransmissionId` = innsendingens
  `forsendelseId` — og vedleggets URL med `consumerType: "Api"` peker
  på `ameldingtilbakemelding.api.skatteetaten-test.no/v1/forsendelser/<den
  transmissionens id>`. Veien er altså
  `GET platform.tt02.altinn.no/dialogporten/api/v1/enduser/dialogs/<dialogId>`
  → finn transmissionen → hent URL-en. Det er det Skatteetaten mener
  med «lytt på hendelser hos Dialogporten»; i drift skal det skje via
  `altinn:events.subscribe` (bruksvilkårene §1.2.4: hendelser, ikke
  polling).
- **Dialogportens sluttbruker-API godtar systembrukertokenet** når
  scope-et er `digdir:dialogporten skatteetaten:innrapporteringamelding`
  og `authorization_details` er det samme — ingen egen pålogging.
- Tokenet for innsending, tilbakemelding og Dialogporten er ett og
  samme systembrukertoken for kunden; bare scopene varierer.

## Status & verification

- Implemented and locked by tests (no credentials needed): grant-JWT
  claims (decoded and checked against the spec), the full token flow and
  its cache against a local mock endpoint, melding XSD validity.
- Requires real test credentials (next step, blocked on client
  registration): live validation round-trip, delegation, submission.
