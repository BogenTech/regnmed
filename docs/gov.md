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

**Status:** the 2026-07-24 scope bestilling via skriv-til-oss was
answered 2026-08-06 with a redirect: that channel does not handle scope
orders, and the reply points to the brukerstøtte desk on the API
documentation's kontakt-oss page — i.e. the same
`eksternjira.sits.no`-tjeneste this document identified on 2026-07-28.
**Nobody is processing the old request**; the bestilling must be
re-sent there, and the redirect is no loss: the July request asked for
`skatteetaten:mvameldinginnsending`, which does not exist (innsending
goes via Altinn 3, see the table below), while the re-send can order
the full verified list in one henvendelse.

Next concrete steps, in order:

1. Create a user account. **There is no registration form anywhere** —
   least of all on the github.io documentation site (looked for there
   2026-08-06, does not exist; the account machinery lives on
   skatteetaten.no/deling/brukeradministrasjon/). Access follows the
   company role: *«Daglig leder, styrets leder eller lignende for
   virksomheten vil automatisk ha tilgang til
   brukeradministrasjonsverktøyet»* —
   <https://skatt.skatteetaten.no/web/sakservice-web/>. Daglig leder
   logs in there representing the org and creates the user account(s).
   Only if someone else is to administer: delegate the Altinn service
   «Brukeradministrasjon – brukerstøtte for bruk av Skatteetatens
   opplysninger» to them first. The created account's first login on
   the desk goes via «Har du glemt passordet ditt?».
2. Send ONE henvendelse ordering the whole «bestilles nå» table below
   for **test**, stating orgnr, environment and the test client-id.
   (Prod is ordered separately when test has proven out.)
3. When granted: add the scopes to the test client in
   Samarbeidsportalen, then run the live validation round-trip
   (step 4 below).

The **test client exists** with an RS256 keypair registered on it; its
id and the rest of the `MASKINPORTEN_*` configuration live outside git
in `~/.config/regnmed/maskinporten-test.env` (docs/secrets.md).

The aksjonærregisteroppgave scope
(`skatteetaten:innrapporteringaksjonaerregisteroppgave`, #43,
docs/aksjonaer.md) rides in the same henvendelse — and additionally
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

Altinns egen dokumentasjon beskriver bare hvordan **tjenesteeiere** får
disse scopene delegert, ikke hvordan et sluttbrukersystem får dem. Vi
har altså **ingen verifisert bestillingsvei** for dem ennå — ikke skriv
en inn her før den er prøvd. Det praktiske første forsøket er å spørre i
samme henvendelse til Skatteetatens brukerstøtte: innsendingsflyten er
deres, så de vet hvilken delegering den krever.

4. Point the env variables above at the test environment and run
   `regnmed mva-melding … --validate`.

For a regnskapsfører acting on behalf of clients, the client company
**delegates** the scope to the accounting firm in Altinn (Altinn
Autorisasjon). This maps 1:1 onto regnmed's engagement model — the same
firm→client relationship, expressed in the government's registry.

## Mva-melding APIs (crates/regnmed-gov/src/mvamelding.rs)

- **Validation** (implemented): POST the melding XML to Skatteetaten's
  validation endpoint (test:
  `https://idporten-api-sbstest.sits.no/api/mva/grensesnittstoette/mva-melding/valider`,
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

## Status & verification

- Implemented and locked by tests (no credentials needed): grant-JWT
  claims (decoded and checked against the spec), the full token flow and
  its cache against a local mock endpoint, melding XSD validity.
- Requires real test credentials (next step, blocked on client
  registration): live validation round-trip, delegation, submission.
