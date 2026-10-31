# Ta med din egen agent

Kundens eget LLM, med kundens egne nøkler, som en ansatt eller en
regnskapsfører i selskapet. Vi hoster ingen modell og selger ingen
tokens — kunden bringer agenten, vi stiller skinnen. Det holder
personvernet, regningen og ansvaret der de hører.

Tre saker, én leveranse: **#101** gir agenten en rolle, **#102** gir den
verktøy, **#103** gir den en jobb å gjøre og en instruks for å gjøre den
godt.

Tilgangsmodellen står i [integrations.md](integrations.md) og
[auth.md](auth.md); dette dokumentet er agentlaget oppå den.

## 1. Prinsippet: en agent er en bruker

Vi fant aldri opp en agent-tilgangsmodell, og det er grunnen til at
dette ble lite. Setningen fra #45 holder uendret:

> Tokenet beviser identitet. Regnmed avgjør hva den identiteten får
> gjøre.

En agent er en `person` med `kind='integrasjon'` og et
`integration_grant` som bærer et **rollenavn** — innebygd eller
selskapets eget. Tilgangsoppslaget, attribusjonen og revisjonssporet er
de samme som for et menneske. Tilbakekalling er én `valid_to` unna og
virker i samme øyeblikk.

Det ene en maskin aldri får, uansett rolle, er en rettighet som styrer
**hvem som har tilgang** (`Right::can_be_delegated`). Den gjør jobben, men
utvider ikke tilgang — heller ikke sin egen.

## 2. MCP-serveren er transport, ikke autorisasjon

`crates/regnmed-mcp` er en tynn adapter foran API-et som alt finnes:

- **Ingen databasetilgang.** Med vilje — en MCP-server med DB-tilgang
  ville vært en ny vei inn i hovedboken, med egne regler å få galt.
  Crate-en avhenger ikke av `regnmed-db`, og det er håndhevet av
  manifestet.
- **Ingen nøkler av sitt eget.** Agenten bærer kundens
  `client_credentials`-klient fra regnid. Serveren henter token og
  glemmer det.
- **Ingen tilgangsmodell.** `integration_grant` avgjør alt.

Protokollen er JSON-RPC 2.0 over stdio, én linje per melding,
håndrullet. Samme valg som for PDF, pain.001, EHF og SAF-T: formatet er
lite, det er en spesifikasjon vi må treffe presist, og en avhengighet
ville ikke gjort det riktigere — bare vanskeligere å se hva vi sender.

Implementert: `initialize`, `ping`, `tools/list`, `tools/call`,
`prompts/list`, `prompts/get`. Protokollversjon `2025-06-18`.

### Oppsett

Serveren konfigureres med miljøvariabler, som alle MCP-servere:

| Variabel | Betydning |
| --- | --- |
| `REGNMED_API` | API-ets basisadresse. Standard `https://api.regnmed.no` |
| `REGNMED_CLIENT_ID` | Klienten kunden registrerte i regnid |
| `REGNMED_CLIENT_SECRET` | Hemmeligheten til samme klient |
| `REGNMED_ISSUER` | IdP-en. Standard `https://id.regnmed.no`; token-endepunktet oppdages |
| `REGNMED_TOKEN_URL` | Overstyrer oppdagelsen |
| `REGNMED_SCOPE` | Valgfritt scope på token-forespørselen |
| `REGNMED_TOKEN` | Et ferdig token i stedet for klientlegitimasjon (test) |
| `REGNMED_COMPANY` | Selskapet verktøyene arbeider for når et kall ikke navngir ett |

Mangler legitimasjonen, **starter ikke serveren**. Det er valgt: en
server som starter uten nøkler feiler først inne i et verktøykall, der
agenten ikke kan skille en konfigurasjonsfeil fra en tilgangsfeil.

Hemmeligheten står i agentens egen konfigurasjon, ikke i dette repoet
([secrets.md](secrets.md) er absolutt).

### Distribusjon

Serveren kjører på kundens egen maskin, ved siden av agenten — ikke i
clusteret. Den bygges med `cargo build -p regnmed-mcp` fra dette repoet.
Et ferdig bygget binær per plattform i GitHub-releasene er neste steg,
ikke lovet her. Av samme grunn står den ikke i
[frugality.md](frugality.md)s budsjetter: de gjelder det som skal leve
innenfor containergrensen på 64 MB, og dette gjør ikke det.

## 3. Verktøyene

`crates/regnmed-mcp/src/verktoy.rs` er en **kuratert** liste på ~30
verktøy, ikke en generert en.

#102 foreslo å generere verktøylisten fra samme rutetabell som
[api.md](api.md), så den ikke kan råtne. Det ble vurdert og forkastet,
av én grunn: **en verktøyliste er en prompt.** To hundre verktøy er den
sikreste måten å få en liten modell til å velge galt, og oppdraget er at
agenten skal gjøre en god jobb på en liten modell.

Den ærlige kostnaden: tabellen *kan* skli fra API-et.
`crates/regnmed-api/tests/grupper/agent.rs` er vakten — den serverer den
ekte ruteren på en ekte socket, peker en ekte MCP-klient mot den, og
kaller **hvert** verktøy i tabellen med et ekte maskintoken. Et verktøy
som ikke er dekket av testen får testen til å feile, og en sti som ikke
finnes gir `404`/`405` der testen krever `400`. (Den fant én med én
gang: `/settings/registrering` er POST-only, og registreringsstatus
leses fra `/settings`.)

Grupper: orientering, lesing av hovedboken, bilagsflyten
(innboks → forslag → bokfør), salg, migrering, og oppgaver.

To konvensjoner er gjentatt i hvert skjema der de gjelder, fordi en
liten modell ikke skal måtte utlede dem: **beløp er heltall øre**
(positivt = debet), og **datoer er `YYYY-MM-DD`**.

### Filer går som sti, ikke som base64

`bilag_last_opp`, `saft_analyser`, `saft_importer` og de to
CSV-importene tar en **sti til en fil på agentens maskin**. Serveren
leser filen selv. Det er valgt: serveren kjører der agenten kjører, og
en base64-kodet PDF gjennom en modells kontekst er både dyrt og
formålsløst.

Binære opplastinger bærer `?sha256=` regnet ut av serveren. API-et
avviser innhold det alt har, så et nytt forsøk etter et tapt svar blir
en no-op i stedet for et andre bilag — #102s idempotenskrav, oppfylt med
mekanismen #48 alt hadde.

### Feil er prosa, ikke koder

Et verktøykall som feiler kommer tilbake som verktøyinnhold med
`isError: true`, med regnmeds egen melding og HTTP-statusen. Det er med
vilje: API-ets feilmeldinger sier hva man skal gjøre («hovedboken
inneholder N bilag utenfor importjournalen …», «selskapet var ikke
mva-registrert … fjern mva-koden, eller oppdater registreringsstatusen»),
og å gjøre dem til en JSON-RPC-feilkode ville kastet bort den eneste
delen en agent kan bruke.

En **protokollfeil** (ukjent metode, ukjent verktøy) er noe annet og
svares som JSON-RPC-feil: det er klientens feil, ikke modellens.

## 4. Rollepromptene

`crates/regnmed-mcp/src/rolle.rs` serverer fire prompter over
`prompts/get`: `regnskapsforer`, `bokforer`, `ansatt` og `migrering`
(den siste er en oppgaveprompt, ikke en rolle). Hver består av felles
grunnregler pluss rollen sin.

De er skrevet for en **liten modell**, og det avgjør formen:

- **Imperativt og konkret.** Ingen «du bør vurdere»; en regel modellen
  må veie er en regel den slipper under press.
- **NEVER-ene først, og uten forbehold.** En liten modell følger et
  flatt forbud pålitelig og et betinget upålitelig.
- **Fast arbeidsrekkefølge.** «Gjør A, så B» overlever en kort kontekst;
  «bruk skjønn» gjør det ikke.
- **Verktøy navngitt presist, enheter gjentatt ved hvert bruk.** Øre og
  kroner er ett sifferskift unna hverandre, og modellen får ingen
  tilbakemelding når den gjetter galt.
- **«Jeg vet ikke» har en konkret handling**: `oppgave_svar` med status
  `sporsmal`. En modell uten et sted å legge usikkerhet dikter opp
  sikkerhet i stedet.

Innholdet er de to doktrinene produktet alt har bygget to ganger:

- **Bilagstolkning (#34): foreslår, bokfører aldri.** Hvert felt bærer
  sin begrunnelse, og «ingen forslag» er et gyldig svar.
- **Attestering (#47): et menneske godkjenner.**

Derfor er **standarden forslagsrett, ikke bokføringsrett**. Agenten kan
få skrivetilgang når eieren vil det — #101 gjør det mulig — men
`krev_for_maskin` finnes for at eieren skal kunne si «alt roboten fører
skal jeg ha sett» uten å måtte velge mellom hjelp og kontroll.

En vakt i kode: `each_role_names_the_tools_its_procedure_depends_on`
krever at hvert verktøy en rolleprosedyre bygger på både **finnes** i
tabellen og er **navngitt** i promptteksten. Å døpe om et verktøy uten
den testen ville stilletiende sendt hver agent etter noe som ikke finnes
— og en liten modell fortsetter å prøve framfor å slutte at instruksen
er gal.

## 5. Oppgaver til en agent

Migrasjon 0064. En agent med rolle og verktøy manglet det enkleste: en
måte å bli **fortalt hva den skal gjøre** på, som ikke er en melding i
en chat eieren mister.

| Tabell | Hva |
| --- | --- |
| `agent_oppgave` | Bestillingen. Uforanderlig når den er gitt. |
| `agent_oppgave_hendelse` | Innsettings-bart spor. Nyeste hendelse gjelder. |

Statusen **beregnes**, lagres aldri — samme regel som
abonnementsstatus og hver saldo i systemet. Ingen hendelse betyr `ny`.

- En endret oppgave er en **ny** oppgave. Ellers kan ingen etterpå se hva
  agenten faktisk ble bedt om.
- `utfort` og `avvist` er **endelige**: en trigger nekter en hendelse på
  en avsluttet oppgave, så et svar kan ikke trekkes tilbake i stillhet.
- `sporsmal` er med vilje **ikke** endelig. En agent som er usikker skal
  kunne spørre og fortsette — alternativet er at den gjetter.
- `avvist` og `sporsmal` krever begrunnelse (DB-sjekk). Et avvist svar
  uten tekst er ikke et svar.

Autorisasjonen er mer presis enn en enkelt rettighet kan uttrykke, og er
derfor skrevet ut framfor å finne opp en ny rettighet:

- **Å GI** en oppgave krever `INTEGRASJON_ADMIN`, som
  `kan_delegeres` nekter en maskin. En agent kan altså ikke skrive sine
  egne ordrer — den ene egenskapen som gjør sporet verdt å lese.
- **Å SVARE** krever `INTEGRASJON_LES` **og** å være den agenten
  oppgaven er stilt til (eller et menneske som kan administrere
  integrasjoner, så en ordre kan trekkes tilbake). En agent svarer ikke
  for en annen agent, og ser ikke en annen agents ordrer — den ville
  handlet på dem.

### Endepunkter

- `GET  /companies/{id}/agent/oppgaver?apne=` — køen
- `POST /companies/{id}/agent/oppgaver` — `{melding, integration_id?, frist?}`
- `GET  /companies/{id}/agent/oppgaver/{oid}` — oppgaven + hele sporet
- `POST /companies/{id}/agent/oppgaver/{oid}/svar` — `{status, notat?}`

I portalen: Administrasjon → Oppdrag og integrasjoner, kortet «Oppgaver
til en agent».

## 6. Tester

| Hva | Hvor |
| --- | --- |
| Agent med `admin` kan migrere, men aldri utvide tilgang | `tests/grupper/integrasjon.rs` |
| Et menneske må attestere det en maskin bokfører | `tests/grupper/attestering.rs` |
| Hvert verktøy i tabellen treffer API-et som finnes | `tests/grupper/agent.rs` |
| Bilaget navngir agenten, ikke et subject | `tests/grupper/agent.rs` |
| En avsluttet oppgave kan ikke gjenåpnes | `tests/grupper/agent.rs` |
| Hvert skjema er gyldig JSON, hver sti-plassholder er deklarert | `regnmed-mcp` (enhetstester) |
| Hver rolle bærer grunnreglene og navngir verktøyene sine | `regnmed-mcp` (enhetstester) |
| `null` i et argument droppes framfor å bli sendt | `regnmed-mcp` (enhetstester) |

De to tilgangsvaktene er **sett feile**: med `machine`-filteret fjernet
fra `Access::has` inviterer roboten medlemmer inn i selskapet, og med
`maskinkrav` fjernet fra bokføringstransaksjonen bokfører agenten et
49-kroners bilag uten at noen har sett det.

## 7. Bevisst utenfor

- **Vi selger ingen tokens.** Kunden bringer modellen og regningen.
  Setningen «vi hoster ingen modell» sto her til 2026-10-29; den er nå
  avgrenset til én av to skinner, se §8. Vi blir en prompt-proxy for
  vertsagenten — vi blir ikke en modellselger.
- **Ingen lagret agentbegrunnelse ennå.** #103 pekte på at et
  agentforslag er en PÅSTAND fra en identifiserbar aktør på et
  tidspunkt, og derfor hører i sporet framfor i en gjenutledning. I dag
  legger agenten begrunnelsen i `agent_oppgave_hendelse` eller i
  bilagsteksten; en egen forslagsflate i innboksen (`POST
  …/inbox/{doc}/forslag`) er ikke bygget.
- **Verktøylisten er ikke generert.** Se §3 for hvorfor, og for vakten
  som betaler for valget.
- **Ratebegrensningen er ikke hevet for agenter.** Standarden er 120
  kall per minutt per integrasjon, per replika. En migrering på 36
  dokumenter treffer den; grensen kan heves per integrasjon
  (`integration.rate_limit_min`), men et uttrykkelig bulk-endepunkt
  finnes ikke.
- **Ingen paginering lagt til.** Flere listeendepunkter henter alt. Det
  er et åpent punkt fra #102, ikke en avgjørelse.

## 8. Vertsagent — den andre skinnen (besluttet 2026-10-29)

Kunden skal kunne opprette en agent **i portalen**, gi den navn og
ansikt, legge inn legitimasjonen til sin egen LLM-leverandør
(OpenAI-kompatibelt endepunkt eller OpenRouter), og snakke med den der —
og agenten skal arbeide med den tilgangen rollen gir den, som en ansatt
eller en regnskapsfører. Målet er at en leder kan sette sammen sin egen
arbeidsstokk.

**Dette er et TILLEGG, ikke en erstatning.** Vi har nå to skinner, og
begge består:

| | Egen agent (§2–§4) | Vertsagent (§8) |
| --- | --- | --- |
| Hvor løkken kjører | kundens maskin | vår worker |
| Hvem ser prompten | bare kunden | vi, i transitt |
| LLM-nøkkel | kundens konfigurasjon | lagret hos oss, kryptert |
| Transport | MCP over stdio | HTTP i portalen |
| Personvern | maksimalt | avhenger av leverandøravtale |

Egen agent blir **ikke** avviklet: den er den personvernmaksimale veien,
og for en kunde som ikke vil at bokføringsdata skal passere oss i det
hele tatt, er den det eneste riktige svaret.

### Hvorfor dette er mindre enn det ser ut

Den dyre halvdelen av «agent som ansatt» var aldri praten — den var å
gjøre en robot trygg å gi ekte tilgang. Den er bygget og i drift:
rollen på `integration_grant` (#101), maskintaket
(`Right::can_be_delegated`), `krev_for_maskin` i attesteringen, 33 kuraterte
verktøy med JSON Schema (`VERKTOY`) og fire rolleprompter skrevet for
små modeller (`rolle.rs`). En vertsagent arver alt dette uendret, fordi
håndhevingen ligger i transaksjonene og i tilgangsoppslaget — ikke i
klienten.

To ting gjenbrukes mer direkte enn ventet:

- **`VERKTOY` ER funksjonskatalogen.** `skjema()` leverer allerede JSON
  Schema per verktøy, som er nøyaktig formatet
  OpenAI/OpenRouter-funksjonskall vil ha. Ingen ny verktøydefinisjon.
- **`agent_oppgave_hendelse` ER meldingstråden.** `sporsmal` er med
  vilje ikke terminal — vakten låser bare etter `utfort`/`avvist` — og
  check-constrainten krever `notat` på den. Agenten spør, mennesket
  svarer, arbeidet fortsetter. Et chat-vindu er en presentasjon av en
  tabell som finnes, ikke en ny datamodell.

### Det som faktisk er nytt

1. **Agentidentitet** — navn (finnes: `integration.navn`) og et ansikt.
2. **Leverandørkonfigurasjon per agent** — endepunkt, modell, nøkkel.
3. **Løkken**, som en worker: hent oppgave → bygg meldinger → kall
   modellen → utfør verktøykall mot vårt eget API med agentens egen
   identitet → skriv `agent_oppgave_hendelse`.
4. **Chat-flaten** over tråden som finnes.

### Tre avgjørelser tatt nå

**Nøkkelen lagres kryptert, og det er et bevisst nytt ansvar.** Det
finnes ingen krypteringsprimitiv i repoet i dag — Stripe-nøkkelen er en
miljøvariabel, ikke en rad. En kundenøkkel per selskap er en ny
ressursklasse og et nytt bruddmål. Alternativene er dårligere: å ikke
lagre den betyr at ingen worker kan arbeide når kunden ikke ser på,
som er hele poenget med en arbeidsstokk; og å bruke vår egen nøkkel og
måle forbruk er tokensalg, som §7 avviser. Valget er derfor kryptert
lagring, med nøkkelen utenfor databasen.

**Leverandøren velges av kunden, og det skal stå i UI-et.** Å sende
hovedbokslinjer til en LLM-leverandør gjør leverandøren til
underdatabehandler for kundens regnskapsdata. OpenRouter er her
*dårligere* enn å gå rett til en leverandør, fordi den kan fordele
videre til mange. Kunden må velge leverandør og se hvor dataene går, på
det tidspunktet agenten opprettes — ikke i en avtaletekst.

**Vertsagent krever attestering som standard.** Et hallusinert bilag i
en append-only kjede ligger der for alltid; det kan bare rettes med et
motbilag, som er riktig men støyende. `krev_for_maskin` ble bygget for
akkurat dette (#47, §5), og for en vertsagent står det PÅ med mindre
eieren uttrykkelig slår det av. En agent bør også ha et tak på kall og
tokens: `integration.rate_limit_min` er den ene spaken som finnes i dag.

### Rekkefølge

1. **Agentidentitet** — navn og ansikt. *Levert 2026-10-29.*
2. **Leverandørkonfigurasjon** med kryptering og dataflyt-opplysning.
   *Levert 2026-10-29, se §8.2.*
3. **Workeren** — gjenbruker `VERKTOY` og `rolle.rs`, skriver hendelser.
   Egen deployment, ikke inne i API-ets 64 MB (frugality.md); NATS
   JetStream finnes alt for denne formen for arbeid (mail-skinnen, og
   `regnid-mail-worker` er mønsteret).
4. **Chat-flaten**.

### Kjente hull, ikke skjult

- ~~**Selvbetjent klientregistrering mangler fortsatt.**~~ **LUKKET
  2026-10-29, se §8.6.** Avgjørelsen som sto åpen er tatt: identiteten
  blir fortsatt per agent, og regnmed utsteder fortsatt ingenting selv —
  men PROVISJONERINGEN flyttet bak et nytt, smalt endepunkt i regnid,
  slik at kunden aldri ser en client_id.
- **Meldingstråden har ingen egen «svar»-status.** Et menneskes svar på
  et `sporsmal` legges inn som `pagar` med notat. Det virker, men tråden
  leses som statusoverganger framfor meldinger. En `svar`-status eller en
  egen meldingstabell retter det når chat-flaten bygges.
- **Ingen bildeopplasting.** Ansiktet er initialer, farge og et ikon fra
  `ikoner.js` — ikke et opplastet bilde. Grunnen står i §8.1.
- **Ingen nøkkelrotasjon.** Byttes `AGENT_SEALING_KEY`, lar ingen lagret
  leverandørnøkkel seg åpne mer, og hver agent må få nøkkelen sin på nytt.
  Se §8.2 — det er en kjent begrensning, ikke en forglemmelse.

### 8.1 Agentidentitet (levert)

Et ansikt er **initialer + en farge + et ikon**, ikke et opplastet
bilde. Tre grunner, i vekt:

1. **Mennesker i regnmed har ikke profilbilde.** `UserMenu.svelte`
   tegner initialer, fordi identitetsdata eies av regnid og speiles ved
   innlogging (#68). En agent med opplastet bilde der et menneske har
   initialer er inkonsekvent i den retningen som er vanskeligst å
   forsvare.
2. **Opplastede bilder er en ny flate å tjene brukerinnhold fra** —
   størrelse, innholdstype, sniffing, hasj-sjekk ved nedlasting. Vi har
   mønsteret (vedlegg, innboks, kvitteringer), men det er en egen
   kostnad for et ansikt.
3. **Gjenkjennelighet er det faktiske kravet**, og farge + ikon +
   initialer gir fem agenter fem tydelig ulike ansikter uten en byte
   bildedata.

Fargen er daisyUI-ens egne semantiske farger, så ansiktet følger temaet
som alt annet (temakontrakten). Den **valideres mot en kjent liste i
Rust** — ikke fordi et fritekstfelt ville vært utrygt i databasen, men
fordi verdien ender som et CSS-klassenavn i portalen, og Tailwind v4
genererer bare klasser den finner i kilden: en dynamisk `bg-{farge}`
ville ikke eksistert i CSS-en. Portalen har derfor en statisk tabell
farge → klassestreng, og serveren slipper ingenting gjennom som ikke
står i den.

Navnet er integrasjonens eget og deles av alle dens oppdrag — akkurat
som `person.name` er personens, ikke per selskap. Redigeres det fra
selskap A, endres det også for selskap B. Det er modellen fra §1 (en
agent er en bruker), ikke en forglemmelse.

Endepunkt: `PUT /companies/{id}/integrations/{iid}` (`IntegrasjonAdmin`)
— navn, kontakt, farge og ikon. Tilgangsnivået endres ikke her: en
rolle byttes ved å trekke tilbake og gi en ny, fordi
`integration_grant` er uforanderlig med vilje (0033-vakten).

### 8.2 Leverandørkonfigurasjon (levert)

Migrasjon 0066 gir `agent_llm`: leverandør, adresse, modell og den
forseglede nøkkelen, per **(integrasjon, selskap)**.

Ikke per integrasjon alene. En integrasjon er ÉN identitet som kan ha
oppdrag i flere selskaper, og navnet deles mellom dem (§8.1) — men en
leverandørnøkkel er noens REGNING og noens valg av databehandler.
Selskap A skal ikke kunne bestemme hvilken modell selskap B-s arbeid
kjører på, og enda mindre sende B sin egen faktura.

Tabellen er **innsettings-bar, nyeste rad gjelder** — samme daterte
mønster som `attestation_policy`, `abonnement_pris` og `sats`. Det
koster ingen ekstra tabell og svarer på spørsmålet som betyr noe
etterpå: hvilken leverandør ble hovedboken vår sendt til, når, og hvem
bestemte det. Å slå av modellen er en ny rad (`aktiv = false`), ikke en
sletting, så sporet fortsetter å si hva som var sant før.

**Nøkkelen har ingen vei ut.** `GET …/integrations/{iid}/llm` svarer med
leverandør, modell og `nokkel_lagret: true` — aldri verdien.
`fetch_llm_key` er en egen funksjon fra `hent_llm` nettopp slik at det
å lese oppsettet for visning ikke kan bli kallet som dekrypterer. Testen
sjekker mot HELE svarkroppen, ikke mot et felt: en lekkasje kommer som
et felt ingen tenkte på. Begge vaktene er **sett feile** — med
klartekstlagring svarer databasesjekken, og med nøkkelen lagt i svaret
svarer lekkasjesjekken.

Forseglingen er `regnmed-core::secret`: XChaCha20-Poly1305, tilfeldig
nonce per forsegling lagret foran chifferteksten. Autentisert, så en rad
noen har endret i databasen FEILER framfor å dekryptere til noe annet —
en forfalsket rad blir en feilmelding, ikke en tulle-API-nøkkel sendt
til en leverandør. Dette er det ene stedet i arbeidsområdet som ikke
håndruller formatet sitt; begrunnelsen står i modulens egen toppkommentar.

`https` kreves på adressen: kundens nøkkel og hovedbokslinjer går over
den. `annet` finnes så en kunde kan peke på sin egen gateway uten at vi
slipper en ny versjon.

**Nøkkelen vår er konfigurasjon, aldri en rad.** `AGENT_SEALING_KEY`, 64
hex-tegn. Navnet er valgt med vilje: den er ikke agentens nøkkel, den er
nøkkelen som FORSEGLER agentens — både leverandørnøkkelen her og
klienthemmeligheten i §8.3. Den het `AGENT_NOKKEL` fram til 2026-10-29,
og det navnet leste som «agentens nøkkel», altså som den ene tingen den
ikke er.

Mangler den, er skinnen AV og portalen sier det framfor å tilby et
skjema som ville lagret en nøkkel vi ikke kan beskytte; er den ugyldig,
STARTER IKKE prosessen — en nesten-riktig nøkkel er en
konfigurasjonsfeil noen ellers finner uker senere, når en agent stille
står uten modell.

Lag den og legg den inn utenfor git (docs/secrets.md er absolutt):

```sh
openssl rand -hex 32
kubectl -n regnmed create secret generic agent-credentials \
  --from-literal=sealing-key=<verdien>
```

Secretet må finnes **FØR** første apply: en `secretKeyRef` til et secret
som ikke finnes gir CreateContainerConfigError, synlig bare i
pod-hendelsene (samme felle som `stripe-credentials`, se `.env.example`).
Prod og test har **hver sin** nøkkel — et brudd i test skal ikke åpne
prods rader. Opprettet i begge 2026-10-29.

**Ta vare på den.** Verdien ble generert rett inn i clusteret og finnes
ingen andre steder. Les den ut og legg den i passordhvelvet:

```sh
kubectl -n regnmed get secret agent-credentials \
  -o jsonpath='{.data.sealing-key}' | base64 -d
```

**ROTASJON FINNES IKKE.** Byttes nøkkelen, er hver lagret
leverandørnøkkel uåpnelig, og hver agent må få sin på nytt gjennom
portalen. Det er levelig nå — ingen agenter er i drift — og det blir
dyrere jo lenger vi venter. Den riktige veien er en nøkkel-id per rad og
en omforsegling som leser med den gamle og skriver med den nye; det er
ikke bygget, og det hører sammen med steg 3.

### 8.3 Workeren (levert)

`crates/regnmed-agent` — eget binær, `regnmed-agent run`. Den tar åpne
agentoppgaver som er stilet til en agent med aktiv modell, kjører dem, og
skriver utfallet i `agent_oppgave_hendelse`.

**Avgjørelsen §8 lot stå åpen er tatt: ingen ny autentiseringsvei.**
Vertsagenten henter token med `client_credentials` som seg selv, nøyaktig
som en agent på kundens egen maskin. `integration_grant`, maskintaket og
attesteringsreglene gjelder uendret, håndhevet der de alltid var. Workeren
kjører sløyfen og holder modellnøkkelen; den holder ingen myndighet.
Alternativet — å la workeren handle «på vegne av» en agent fordi den er
innenfor tillitsgrensen vår — ville lagt en andre dør inn i hovedboken, og
hele §1 finnes for å slippe det.

Prisen er at agenten trenger en klienthemmelighet. Migrasjon 0067 legger den
på `integration`, forseglet med samme nøkkel som modellnøkkelen.
Oppdaterbar, ikke innsettings-bar som `agent_llm`: et spor over forkastede
leverandørVALG er bevis verdt å ha, et spor over forkastede HEMMELIGHETER er
en forpliktelse.

**Begge skinnene kjører samme kode.** Verktøykatalogen bygges av `VERKTOY`,
systemprompten av `rolle::prompt`, og kallet går gjennom
`Server::execute_tool` — samme dispatcher som MCP-serveren. Transporten
skiller; oppførselen skal ikke, og den sikreste måten å få to skinner til å
oppføre seg likt er å gi dem samme kode. Testen fester det: katalogen har
nøyaktig like mange verktøy som tabellen.

Sløyfen stopper etter `MAX_TURNS` (12) og SIER at den stoppet. En sløyfe
som ikke stopper bruker kundens penger; en agent som stille gir opp er verre
enn en som melder fra om en vegg.

Modellen avslutter med en markør — `FERDIG:`, `SPØRSMÅL:` eller `AVVIST:` —
som blir statusen køen alt kjenner (#103). Workeren finner ikke opp egne.
Tekst uten markør leses som utført: en liten modell som gjorde jobben og
glemte prefikset skal ikke få arbeidet sitt journalført som en nektelse, og
notatet bærer råteksten uansett. `sporsmal` er ikke terminal i 0064-vakten,
nettopp fordi agenten skal kunne fortsette når noen har svart.

Leverandøren er OpenAI-kompatibel `/chat/completions` med `tools` —
grunnen til at dette ble lite. OpenAI, OpenRouter og de fleste
selvhostede gatewayer snakker samme protokoll, så én klient dekker det
kunden sannsynligvis tar med, og `annet` dekker resten uten en ny versjon
fra oss. Felter vi ikke modellerer ignoreres framfor å avvises: dette er en
wire vi ikke eier.

**Verifisert end-to-end mot en falsk leverandør** (`agent.rs`): modellen ber
om `saldobalanse`, workeren kjører det mot det EKTE routeren med agentens
eget token, mater svaret tilbake og rapporterer `utfort`. Testen sjekker at
verktøysvaret bærer data fra API-et — og den er **sett feile**: med
dispatcheren kortsluttet til å svare «ok» uten å kalle noe, sier den fra.

#### Ærlige begrensninger

- **Kjørt mot ekte leverandør 2026-10-29**, og det fant to feil den falske
  ikke kunne finne (se §8.4). Lærdommen fra Stripe-integrasjonen (punkt 56 i
  CLAUDE.md) holdt: payloads skrevet ut fra hva koden LESER bekrefter bare
  kodens egen antakelse.
- **Utrullet 2026-10-29** som CronJob hvert femte minutt
  (`deploy/shared/agent-worker.yaml`), `concurrencyPolicy: Forbid` fordi
  oppgavene ikke låses mens de kjøres — to samtidige kjøringer kunne
  plukket samme oppgave og kjørt modellen to ganger på kundens regning.
  `regnmed-agent` har eget budsjett i [frugality.md](frugality.md)
  (8 MB, målt 5,4). Uten åpne oppgaver spør kjøringen databasen og
  avslutter; den rører ikke leverandøren.

  To ting å vite fra selve utrullingen. (a) `.dockerignore` ekskluderer
  alt og slipper inn navngitte stier, så en ny `COPY` i Dockerfile uten
  en tilsvarende linje der feiler bygget på en fil som nettopp ble
  kompilert — fila gjør jobben sin, men feilmeldingen peker på Docker
  framfor på `.dockerignore`. (b) **Manifestet nådde test før imaget som
  inneholdt binæret**: Flux applyer `deploy/test` fra main med den
  image-pinnen som står der NÅ, så tre planlagte kjøringer feilet på et
  image uten `/app/regnmed-agent` før neste pin kom. Vinduet lukker seg
  selv, men en ny CronJob for et nytt binær bør ikke landes før imaget
  som bærer det.
- **Ingen strømming.** Oppgaven kjøres ferdig før noe skrives. Chat-flaten
  (steg 4) vil ville ha delsvar.
- **Ingen kostnadsgrense.** `MAX_TURNS` begrenser antall turer, ikke antall
  tokens. `integration.rate_limit_min` begrenser kall mot VÅRT API, ikke mot
  leverandøren.
- **Selvbetjent klientregistrering mangler fortsatt.** Klienthemmeligheten
  må komme fra en operatør som kjørte `regnid add-client`. Det er samme hull
  §8 åpnet med, og det er nå i veien for den vanlige kunden.

### 8.4 Hva den ekte leverandøren fant

`crates/regnmed-api/tests/ekte_leverandor.rs` er en håndkjørt prøvebenk mot
OpenRouter — no-op uten `OPEN_ROUTER_KEY`, siden den koster penger. Den fant
to feil som den falske leverandøren ikke kunne finne, og det er grunnen til
at den finnes.

**(a) Et ubesvart spørsmål lukket oppgaven.** `gemini-2.5-flash-lite` spurte
hvilken dato i 2026 som var ment, uten `SPØRSMÅL:`-prefikset. Fallbacken
leste umerket tekst som utført — og `utfort` er TERMINAL i 0064-vakten, så
oppgaven ble lukket og spørsmålet forsvant med den. Umerket tekst som ender
med spørsmålstegn tolkes nå som `sporsmal`. Asymmetrien avgjør regelen: å
være feilaktig IKKE-terminal er gjenopprettelig (et menneske leser notatet
og lukker), å være feilaktig terminal er det ikke.

**(b) En bokfører kunne ikke føre et bilag.** Bedt om å bokføre en kontant
utgift svarte `gpt-4o-mini`: «Jeg kan ikke bokføre bilaget fordi jeg ikke
har et vedlegg tilgjengelig for opplasting.» Den hadde rett — tabellens
eneste posteringsverktøy var `bilag_bokfor`, som bokfører et
INNBOKSDOKUMENT. Ruten for manuell føring (#70, `POST …/vouchers`) fantes i
API-et og manglet i tabellen. Det er kostnaden ved en KURATERT liste (§3),
og `agent.rs` kunne ikke se den: den sjekker at verktøyene i tabellen
treffer API-et, ikke at API-ets viktige ruter er i tabellen. Nytt verktøy
`bilag_for`, og det UTØVES i `agent.rs` framfor å stå på unntakslisten —
det skriver til hovedboken, det ene stedet et sklidd verktøy gjør skade i
stedet for bare å feile.

Verifisert etterpå, mot ekte modell: bilag 2026-1 bokført, 50 000 øre debet,
lest tilbake FRA HOVEDBOKEN og ikke fra modellens egen påstand.

Observert underveis, ikke feil: to 8B-modeller er borte fra OpenRouter
(`qwen/qwen3-8b` avviklet 9. okt 2026, `mistralai/ministral-8b` uten
leverandør). Leverandørens egen feilmelding gikk uendret ut i oppgavens
notat, som er oppførselen §8.3 lover — kunden leser «was deprecated», ikke
«noe gikk galt».

Kjente modeller som gjorde jobben: `openai/gpt-4o-mini`,
`qwen/qwen3.7-flash`, `meta-llama/llama-3.1-8b-instruct`,
`google/gemini-2.5-flash-lite` (som spør oftere enn den gjør).

### 8.5 Samtalen (levert) — steg 4

Chat-flaten er en VISNING av `agent_oppgave_hendelse`, ikke en ny
datamodell. Tråden fantes fra #103, fordi `sporsmal` med vilje ikke er
terminal i 0064-vakten. Derfor finnes det ingen «slett melding» her:
sporet er innsettings-bart, og et svar kan ikke trekkes tilbake.

**Ingen ny status.** §8 noterte at tråden mangler en egen «svar»-status, og
at et menneskes svar lander som `pagar` med notat. Det viste seg å være
RIKTIG framfor et hull: `pagar` sier at arbeidet fortsetter, notatet er det
som ble sagt, og `av` sier hvem som sa det — alt vi trenger for å tegne to
sider. Og det gjør oppgaven plukkbar igjen for workeren, som hopper over
`sporsmal`. En ny status ville kostet en migrasjon og et nytt tilfelle i
hver match, for ingenting.

**Tråden mates tilbake til modellen.** Dette var det virkelige hullet, og
det var ikke i UI-et: `run_loop` fikk bare bestillingen, så en agent som
hadde spurt og ble plukket opp igjen ville spurt om det samme for alltid —
og da hadde det ikke hjulpet at `sporsmal` er ikke-terminal.
`run_task` bygger nå replikkene av historikken: hendelser skrevet av
agentens eget navn blir `assistant`, resten `user`, og en bar `pagar` uten
notat («begynte å arbeide») er ingen replikk. Testfestet i `agent.rs`:
systemprompt, bestilling, agentens eget spørsmål som `assistant`, og
menneskets svar som `user` — i den rekkefølgen.

**Boblene er håndrullet.** daisyUIs `chat`-komponent kostet ~2,3 KB i dist
for ren utsmykking, og budsjettet (frugality.md) hadde 7 KB igjen; flex og
`rounded-box` gjør samme jobb med klasser bundelen alt har. Samme avveining
som ikonene (#67): vi skriver markupen, temaet eier fargene. Fargene er
`bg-primary` for mennesket, `bg-base-200` for agenten, `bg-warning` for et
spørsmål og `bg-error` for en avvisning — statusen bærer fargen, så en
oppgave som venter på deg er synlig uten å leses.

Verifisert i nettleseren hele veien: agenten spør, svaret sendes fra
kortet, nyeste status blir `pagar`, og oppgaven er plukkbar av workeren
igjen (sjekket i databasen, ikke i UI-et).

#### Aldri kjørt ende-til-ende i clusteret

Workeren STARTER i test, leser den forseglede nøkkelen, spør databasen og
avslutter rent — det er verifisert. Det som ikke er verifisert er en agent
som faktisk ARBEIDER der: det krever en `client_credentials`-klient i test
sin regnid og at noen logger inn i portalen og setter opp agenten, og begge
delene er menneskearbeid som ikke kan gjøres herfra. Sløyfen selv er kjørt
mot ekte modeller (§8.4), men ikke med token hentet i clusteret.

#### Fortsatt ikke gjort

- **Ingen strømming.** Oppgaven kjøres ferdig før noe skrives, så et svar
  dukker opp i ett stykke. Det merkes mest på en lang oppgave.
- **Ingen varsling.** Et spørsmål venter til noen åpner kortet. Mail-skinnen
  finnes (#32/#35) og er den åpenbare veien, men den er ikke koblet.
- **Ingen ulest-markering.** Du ser `spør`-merket i tabellen; du ser ikke
  hva som er nytt siden sist.

### 8.6 Agenten opprettes i portalen (levert 2026-10-29)

Fram til nå endte hver agent med det samme: «kjør denne kommandoen på
serveren vår». Det er ikke en liten friksjon, det er en sperre — en
regnskapsfører som vil ha en robot kan ikke bes om å skrive
`regnid add-client --grant-type client_credentials --confidential`, og
en funksjon ingen tar i bruk er ikke levert.

**Innsigelsen mot å fjerne CLI-en var feil på ett punkt.** Den var at
klientregistrering er «en egen, mer overveid handling». Det stemmer
fortsatt. Men handlingen er overveid fordi NOEN HAR BESTEMT at den skal
kunne skje — ikke fordi et menneske skriver den inn hver gang. Så
overveielsen flyttet opp ett nivå: regnid stoler på nøyaktig de
klientene som står i `PROVISION_CLIENTS`, og ingen andre. Det er
driftskonfigurasjon, ikke data; en angriper som kommer inn i databasen
når den ikke.

Dette er samme form som LLM-nøkkelen alt hadde. Den er også en
systemlegitimasjon brukeren limer inn i et skjema og aldri tenker på
igjen. Klient-id-en var det samme slaget ting, og ble behandlet som noe
kunden måtte forstå.

#### Det som IKKE endret seg

Mest av alt dette: **agenten har fortsatt sin egen identitet.** Den
henter token som seg selv, med `client_credentials`, og går gjennom
`integration_grant`, maskintaket (`Right::can_be_delegated`) og
attesteringsreglene som før. Snarveien vi vurderte og forkastet —
ÉN privilegert legitimasjon som workeren bruker til å opptre som hvilken
som helst agent — ville gjort én lekkasje til alle agenter i alle
selskaper. Den står fortsatt forkastet.

#### regnid: `POST /provision/machine-client`

Autentisert med HTTP Basic (klient-id + hemmelighet), og kalleren må stå
i `PROVISION_CLIENTS`. Listen sjekkes FØR hemmeligheten, så en klient som
ikke får provisjonere lærer ikke om den gjettet riktig.

Det den kan lage er smalt ved konstruksjon: `client_credentials` bare,
konfidensiell bare, ingen redirect-URI-er. En stjålet
provisjoneringshemmelighet kan altså ikke bli et innloggingsendepunkt —
den kan bare lage roboter, som relying party-en så må gi tilgang før de
kan noe som helst. `tests/provision.rs` i regnid fester alle fire
avslagene, og allowlist-vakten er SETT FEILE.

`audience` følger med fra regnmeds egen `OIDC_AUDIENCE`, lest gjennom
`Verifier::audience()` i stedet for en konstant: en klient uten riktig
`aud` logger seg pent inn og får så 401 på hvert eneste kall, uten spor
noe sted (CLAUDE.md punkt 56).

#### regnmed: `POST /companies/{id}/agents`

Ett kall lager hele agenten — identitet, tilgang, ansikt, instrukser og
modell. `client_id` er valgfri: oppgis den, brukes den (agenten kunden
kjører selv, skinne 1), ellers lages den.

Rekkefølgen er valgt etter hva et halvferdig kjør etterlater seg.
IdP-klienten lages FØRST, fordi en maskinklient uten grant kan hente
token og ikke gjøre noe som helst — den er inert. Motsatt rekkefølge
ville etterlatt kunden med en agent i sin egen liste som aldri kan
autentisere.

Konfigurasjon: `REGNID_PROVISION_CLIENT_ID` + `REGNID_PROVISION_SECRET`
(begge eller ingen, kortskinne-regelen), og `REGNID_BASE_URL` når IdP-en
ikke nås på issuer-adressen. Uten dem spør portalen om en client_id som
før — den gamle veien er fortsatt der, bare ikke lenger normalen.

#### Egne instrukser per agent (migrasjon 0068)

`integration.systemprompt`: tekst som legges til ETTER rolleprompten og
FØR avslutningsreglene. Rekkefølgen betyr én ting og bare én — den lar
instruksene spesialisere («strøm bokføres på 6340», «spør Aud ved tvil»)
uten å fortrenge hverken rollens regler eller markørene `tolk()` leser.

Den betyr IKKE noe for tilgang. Hva en eier enn skriver her, er rollen
på grantet det `access::require` håndhever på hvert kall agenten gjør: det
verste en dårlig instruks får til er en agent som prøver ting den blir
nektet. Grensen ligger i oppslaget, aldri i prompten.

Taket er 4000 tegn, og det er en kostnadsgrense, ikke en smaksdom: hele
prompten sendes med HVER modelltur, så en roman her er kundens egen
regning, tur etter tur.

#### Agentene står i Brukere og roller

Kortet flyttet fra Oppdrag til Administrasjon → Brukere og roller.
Begrunnelsen er §1s egen setning: en agent ER en bruker. Da er «hvem har
tilgang her, og som hva?» det samme spørsmålet for en robot som for en
ansatt, og det skal stilles på én side.

Rollen kan endres der den vises (`PUT …/integrations/{iid}/rolle`).
Under panseret er det fortsatt tilbakekalling pluss et nytt grant, i ÉN
transaksjon — et grant er uforanderlig (0033), så den gamle rollen blir
stående sann for dagene den gjaldt. Samme rolle to ganger avvises: ellers
ville hver lagring lagt på en rad og sporet sluttet å bety noe.

**Funnet i nettleseren, ikke i testene:** etter et rollebytte står samme
agent med to rader, og tabellen var nøklet på `integration_id`. To like
nøkler kaster i en Svelte-mal, og et ukastet unntak der FRYSER
komponentens reaktivitet — kortet ble dødt, ikke feil (CLAUDE.md punkt
68, andre gang). Listen bærer nå `grant_id`, og testen fester at hver
rad har sin egen nøkkel mens `integration_id` er felles.

#### Fortsatt ikke gjort

- **Ingen nøkkelrotasjon for provisjoneringshemmeligheten.** Byttes den,
  må `PROVISION_CLIENTS` og regnmeds config endres sammen.
- **En mislykket provisjonering etterlater ingenting hos kunden, men kan
  etterlate en ubrukt klient i regnid.** Den er inert (ingen grant),
  og `[provision]`-loggen navngir den, men ingen rydder den bort.
- **Ingen agent har kjørt en oppgave ende til ende i et cluster.**
  Uendret fra §8.3. Det som er nytt her er hvordan agenten BLIR til.

### 8.7 Arbeidskø i stedet for CronJob (levert 2026-10-29)

CronJobben kjørte hvert femte minutt med `concurrencyPolicy: Forbid`.
Det betydde ÉN agentoppgave om gangen for HELE plattformen — ikke én per
selskap. En bokfører som malte seg gjennom tolv turer blokkerte alle
andres agenter til den var ferdig eller traff 900-sekundersgrensen.

Det er ikke en forsinkelse man justerer bort. Det er et tak på én, og
«hvert femte minutt» var aldri den egentlige kostnaden.

NATS JetStream stod alt i begge clusterne og var alt en avhengighet
(`regnmed-mail`), så dette krevde ingen ny infrastruktur.

#### Køen er en DØRKLOKKE, ikke fasiten

`agent_oppgave` i databasen er fortsatt autoritativ: den vet hvilke
oppgaver som står åpne og hva som skjedde med dem. En melding på
`regnmed.agent.oppgave` sier bare «se nå» i stedet for «se innen fem
minutter». To konsekvenser, begge med vilje:

1. **En tapt melding koster VENTETID, ikke arbeid.** Feiebrettet —
   `regnmed-agent run`, timelig CronJob — finner oppgaven uansett. En
   NATS-nedetid setter altså agentene tilbake til oppførselen de hadde
   før køen fantes, framfor å stoppe dem.
2. **En duplisert melding koster INGENTING**, fordi workeren tar en lås
   i databasen før den gjør noe. At-least-once er greit når det som
   leveres er et puff.

Meldingen bærer bare id-er. Hva agenten leser og skriver blir mellom
workeren og API-et; å legge regnskapsdata på en delt strøm ville utvidet
hvor de dataene ligger lagret uten å gi noe — workeren må slå opp i
databasen uansett.

#### Låsen (migrasjon 0069)

Hvorfor en lås når en work queue alt leverer til én konsument? Fordi
`ack_wait` må være lang — en modelltur tar minutter — og en
re-levering etter at den utløper ville satt worker nummer to på en
oppgave worker nummer én fortsatt kjører. **Den oppgaven bokfører
bilag.** At-least-once pluss en ubeskyttet hovedbokskriving er nøyaktig
kombinasjonen dette systemet finnes for å hindre.

`agent_task_claim` er derfor et LEIEFORHOLD, ikke et spor: raden
slettes når oppgaven er ferdig. Hva agenten gjorde står i
`agent_oppgave_hendelse`, som er innsettings-bar og forblir det.

Å ta låsen er ÉN setning (`insert … on conflict do update … where
claimed_at < now() - interval`), altså atomisk: enten holder du raden
eller så gjør noen andre det, og ingen rekkefølge av lesere kan få to
til å tro de vant. Testen kjører ti samtidige forsøk og krever nøyaktig
én vinner — **sett feile** med den åpenbare lese-så-skrive-varianten.
En lås eldre enn `CLAIM_TTL_MINUTES` (15) kan overtas, ellers ville en
worker som ble drept parkert oppgaven for alltid.

Feiebrettet tar samme lås. Uten det ville nettopp det stedet vært det
ene som kunne kjøre en oppgave to ganger — og helst når et etterslep
gjør det mest sannsynlig.

#### Samtidige verktøykall i én tur

Verktøykall fra ÉN tur er uavhengige av hverandre: modellen ba om alle
før den så noe svar. De kjører nå fire om gangen (`CONCURRENT_TOOLS`),
bundet fordi API-et ratebegrenser per integrasjon — et ubundet byks
ville gitt agenten en 429 og brukt en tur på å bli irettesatt.

Dette er den ENESTE parallelliteten én oppgave kan ha. Turer kan ikke
overlappe, for en turs inndata ER forrige turs utdata; å sende turer på
en kø ville bare vært stafett mellom workere, med samme klokketid og
regnskapsdata på kjøpet. Samtidighet PÅ TVERS av oppgaver er det køen
gir: ti agenter for ti selskaper kjører faktisk samtidig.

Stafetten finnes forresten alt, der den lønner seg: når agenten stiller
et spørsmål, slippes workeren, og hvilken som helst worker kan ta
oppgaven igjen når et menneske svarer — tråden bygges opp fra
`agent_oppgave_hendelse`, ikke fra minnet. Overleveringen skjer altså
når man venter på et MENNESKE, som tar timer, ikke på en modell, som
tar sekunder.

#### Drift

`regnmed-agent listen` er en Deployment; replikatallet er taket på
samtidige agentoppgaver (test 1, prod 3). `regnmed-agent run` er
feiebrettet, timelig. Workeren krever `NATS_URL` og nekter å starte uten
— en lytter uten kø ser frisk ut og gjør ingenting.
