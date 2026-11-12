# Aksjeeierbok og aksjonærregisteroppgave (RF-1086)

To ting som ofte blandes sammen, og som regnmed holder fra hverandre:

1. **Aksjeeierboken** — et lovpålagt register selskapet fører selv
   (aksjeloven §4-5). Det har verdi hver dag, uavhengig av
   innrapportering.
2. **Aksjonærregisteroppgaven (RF-1086)** — den årlige innrapporteringen
   til Skatteetaten, frist **31. januar** (skatteforvaltningsloven §7-7).

Den første er kilden til den andre. Ingen tall tastes inn to ganger.

## Hvorfor dette haster nå

> **Fra juni 2026 kan aksjonærregisteroppgaven bare leveres gjennom et
> sluttbrukersystem.** Altinn.no og papir er avviklet, også for
> endringsoppgaver for 2025 og tidligere år.

Kilde: [Skatteetaten om
aksjonærregisteroppgaven](https://www.skatteetaten.no/bedrift-og-organisasjon/rapportering-og-bransjer/aksjonarregisteroppgaven/).
Det gjør dette til en ren *må*-funksjon for ethvert AS regnmed skal
betjene, ikke en bekvemmelighet.

Selskaper registrert i Euronext VPS er unntatt — der rapporterer VPS.

## Aksjeeierboken: beregnet, aldri lagret

Eierandelen ligger ikke i en kolonne noen overskriver. Den er summen av
**hendelser** fram til en dato, nøyaktig som en kontosaldo er summen av
bilag:

| | Hovedboken | Aksjeeierboken |
| --- | --- | --- |
| Grunnenhet | bilag med linjer | hendelse på en aksjonær |
| Uforanderlighet | append-only + hashkjede | insert-only (trigger) |
| «Nåtilstand» | `SUM(amount_ore)` | `SUM(±antall)` |
| Retting | motsatt bilag | motsatt hendelse |

Migration 0034 gir tre tabeller:

- **`shareholder`** — identiteten (fødselsnummer / organisasjonsnummer /
  utenlandsk aksjonær-ID) er **permanent**, håndhevet av trigger *og*
  kolonnerettigheter. Et bytte av identitet er en ny aksjonær, ikke en
  redigering. Kontaktopplysninger er redigerbare, for folk flytter.
  Aksjonærer slettes aldri.
- **`share_event`** — insert-only. `antall` er alltid **positivt**;
  retningen ligger i typen, så en rad kan ikke motsi seg selv ved å
  påstå et negativt kjøp. En overdragelse skrives som **to** rader
  (avgang hos selger, tilgang hos kjøper) i **én** transaksjon, med
  `motpart_id` begge veier — aksjeeierboken kan aldri inneholde halve
  salg.
- **`dividend`** — **ett vedtak, ikke ett beløp per eier.** Den enkeltes
  utbytte er antall aksjer på beslutningsdatoen ganger utbytte per
  aksje. Da kan summen av delene per konstruksjon ikke avvike fra
  helheten, og selskapsnivået (post 21) og aksjonærnivået stemmer
  automatisk overens. Vedtaket bokføres (2050 → 2800) i **samme
  transaksjon** som det registreres.

`regnmed-core::aksjebok` eier foldingen (`beholdning`,
`aarsbevegelse`) og transaksjonstypene. Databasen utleder retningen fra
den samme listen, så de to kan ikke drifte fra hverandre.

## Fødselsnummer: én vei inn, én vei ut

Dette er en bevisst personvernavgjørelse, ikke en tilfeldighet:

| | Krever | Vises i |
| --- | --- | --- |
| Aksjeeierboken (§4-5) | **fødselsdato** | portalen, API-listingen |
| RF-1086 | **fødselsnummer** | kun innsendingsfilen |

Aksjeeierboken skal etter §4-5 være tilgjengelig for enhver. Loven ber
der om fødselsdato, ikke fødselsnummer. Derfor lagrer vi nummeret fordi
innrapporteringen krever det, mens `Aksjonaer`-strukturen som portalen
og API-et ser bare bærer **fødselsdatoen**, utledet av
`regnmed-core::nin`. Nummeret leses ett sted — når oppgaven bygges — og
integrasjonstesten fester det: aksjeeierbok-svaret skal ikke inneholde
nummeret, innsendingen skal.

`regnmed-core::nin` validerer også kontrollsifrene (MOD11, to runder) og
leser ut fødselsdatoen med de tre forskyvningene som finnes i omløp:

- **D-nummer**: dagen + 40 (utenlandsk aksjonær uten personnummer).
- **H-nummer**: måneden + 40 (hjelpenummer fra helsevesenet).
- **Syntetisk nummer**: måneden + 80 — Skatteetatens egen konvensjon for
  testpersoner (Tenor). Vi *må* lese dem: testmiljøet krever syntetiske
  data, så en parser som avviste dem kunne aldri testes mot det ekte
  API-et. Etatens eget RF-1086-eksempel bruker et slikt nummer.

Århundret kommer fra individnummeret lest sammen med årstallet — regelen
som skiller en aksjonær født i 1905 fra en født i 2005.

## Oppgaven: to skjemaer

`regnmed-core::aksjonaeroppgave` rendrer **hand-rolled og
deterministisk**, som SAF-T, mva-meldingen, pain.001 og EHF:

- **Hovedskjema** (RF-1086, skjemanummer 890): selskapets tall.
- **Underskjema** (RF-1086-U, skjemanummer 923): ett per aksjonær.

Formatet er Altinns `Skjema`-dialekt — hver gruppe bærer sin `gruppeid`,
hvert felt sin `orid`, og rekkefølgen er XSD-ens egen sekvens. Beløp er
heltall øre helt fram til formatteringen; ingen flyttall er innom.
Datoer er `xs:dateTime` ved midnatt, slik etatens eksempel skriver dem.

Skatteetatens offisielle XSD-er er vendored i `docs/aksjonaer/` og
kjøres med xmllint i **både** enhetstester og integrasjonstest, og i CI.

Aksjonærnavn er begrenset til 35 tegn i skjemaet. Et langt selskapsnavn
**kortes ned** framfor å stoppe en ellers riktig levering — identiteten
ligger i organisasjonsnummeret, ikke i navnet.

### Transaksjonskodene (verifisert 2026-11-11)

Hver bevegelse rapporteres med en **transaksjonstypekode**
(`AksjeErvervType` i post 23, `AksjerArvMvOmsattType` i post 25,
`AksjerNyutstedteStiftelseMvType` i post 9). Tre kilder, alle
Skatteetatens egne:

1. **XSD-ene** (vendret i `docs/aksjonaer/`, hentet fra
   Skatteetaten/api-dokumentasjon 2026-11-11) har nå én `enumeration` per
   post — `KodelisteErvervtype` (post 23), `…_TransTyper_p25` osv. — og
   API-et avviser alt utenfor dem (`cvc-enumeration-valid`, prøvd med
   `XX`). XSD-ene gir **koder, ikke navn**.
2. **«Detaljert design»** for RF-1086 og RF-1086-U (2024, regneark på
   skatteetaten.no/samarbeidspartnere/sluttbrukersystemer/
   aksjonarregisteroppgaven-sbs/) parer kodene med navn i
   valideringsreglene, f.eks. «Ved 'Arv/gave m/skattem. kontinuitet' (GK)
   må avgivers fødselsnummer …».
3. **Eksempelfilene** på samme side (`stiftelseavselskap_*`,
   `kjopogsalg_*`, `skattemessig_kontinuitet_*`, …).

En kode legges inn i `TransactionType::code()` **bare når en av disse
navngir den**, med kilden i en kommentar:

| Type | Kode | Kilde |
| --- | --- | --- |
| stiftelse | `T` | eksempel `stiftelseavselskap_*`; design (post 9) |
| nyemisjon | `N` | design |
| nyemisjon ansattaksjer | `NAK` | design |
| kjøp / salg | `K` / `R` | eksempel `kjopogsalg_*`; design VAL_45 |
| bytte av aksjer til/fra selskap utenfor Norge | `AE` | design |
| arv/gave med skattemessig kontinuitet (begge sider) | `GK` | design VAL_13, VAL_24 |
| avgiftspliktig arv/gave med kontinuitet (begge sider) | `AK` | design VAL_56, VAL_57 |
| overføring med skattemessig kontinuitet (begge sider) | `OK` | eksempel `skattemessig_kontinuitet_*` |
| fordeling mellom ektefeller ved skilsmisse (begge sider) | `ES` | design VAL_56, VAL_57 |

**En feil vi fant og rettet:** regnmed rapporterte stiftelse som `N`,
etter eksempelet i API-dokumentasjonen. Både etatens egne eksempelfiler
for stiftelse og designets kontrollmelding bruker `T`, og `N` er
nyemisjon. `N` er en gyldig kode, så API-et tok imot det uten å si fra —
det er nettopp den feilen en høylytt nektelse skal hindre.

**Overføringer med kontinuitet** krever motpartens identitet
(designets VAL_13/56/24/57): mottakeren oppgir avgivers fødselsnummer
(`26530`) eller org.nr (`26531`), avgiveren oppgir mottakers (`26532`/
`26533`) — `AK` og `ES` bare fødselsnummer. Aksjeeierboken fikk derfor
egne **avgangstyper** for de fire (`…_avgang`, samme navn og kode), og
typelisten forteller portalen hvilken type motparten får
(`motpart_type`). Mangler motpartens nummer, nekter rendringen.

**Fortsatt uten kode, med vilje:**

- **Ingen kilde navngir koden:** arv og gave *uten* kontinuitet (koden er
  etter alt å dømme `A`/`G`, men ingen kilde sier det), konsernintern
  overføring, nyemisjon ved konvertering av fordring og ved konsernintern
  overføring, stiftelse/nyemisjon med inntektsfradrag (`TH` heter
  «Fradrag for investering i oppstartsselskap» — trolig det samme, men
  ikke med det navnet).
- **Koden er navngitt, men hovedskjemaets side er ikke bygget:** fusjon
  og fisjon (`UP`/`IP`, `U`/`I`), omdanning (`M`), flytting (`FS`),
  likvidasjon (`L`, `PLS`), sletting av egne aksjer (`W`), spleis (`V`)
  og hele post 24 (fondsemisjon `E`, splitt `S`, sammenslåing `SD`).
  Disse hendelsene skal også stå på selskapsnivå, og en oppgave der
  underskjemaet sier én ting og hovedskjemaet ingenting, er feil selv om
  koden er riktig.

**Derfor gjetter vi ikke.** Transaksjonstypen flyter videre inn i
aksjonærens **egen** aksjeoppgave (RF-1088) og bestemmer inngangsverdi
og skjermingsgrunnlag. En høylytt nektelse er noe man kan rette; en
stille feilrapportert transaksjon er det ikke. Forhåndsvisningen viser
fortsatt tallene og lister hindringene under `leverbar: false`; bare
`format=xml` og innsendingen stopper.

Testen `only_verified_codes_exist` teller kodene og krever at hver
av dem står i XSD-ens liste for posten den rapporteres i.

## Innsending (#182)

Skatteetatens API (OpenAPI 1.3.0, SwaggerHub
`skatteetaten/innrapportering-aksjonaerregister-api`) tar én oppgave i
tre steg, hvert med sin egen `idempotencyKey`:

1. `POST {base}/{inntektsår}/1086H` — hovedskjemaet (XML). Svarer med
   `hovedskjemaId`.
2. `POST {base}/{inntektsår}/{hovedskjemaId}/1086U` — ett underskjema per
   aksjonær (XML).
3. `POST {base}/{inntektsår}/{hovedskjemaId}/bekreft?antall_underskjema=N`
   — lukker oppgaven og svarer med **leveransebekreftelsen**
   (`dialogId`, `forsendelseId`, `oppgavegiversLeveranseReferanse`).

Hvert kall bærer **systembrukertokenet for selskapet**
(`TokenProvider::token_for_systemuser`, scope
`skatteetaten:innrapporteringaksjonaerregisteroppgave digdir:dialogporten`).
Klienten er
`regnmed-gov::shareholder_return`; base-URL-en er testmiljøet
(`https://api-test.sits.no/api/aksjonaerregister/v1`) med mindre
`AKSJONAERREGISTER_URL` sier noe annet — produksjon velges, man havner
ikke der.

**Verifisert mot Skatteetatens testmiljø 2026-11-11**: en oppgave for det
syntetiske Tenor-selskapet 311032240 med stiftelse (`T`), salg (`K`/`R`)
og overføring med skattemessig kontinuitet (`OK` på begge sider, med
motpartens fødselsnummer) ble tatt imot og **godkjent** av Skatteetaten
etter om lag halvannet minutt
(`crates/regnmed-gov/tests/shareholder_return_live.rs`, kjøres for hånd
med `AKSJONAER_LIVE=1`; testen venter på svaret og krever godkjenning).

### Svaret: godkjent eller avvist

Skatteetaten svarer ikke i innsendings-API-et, men i **Dialogporten**:
kvitteringens `dialogId` peker på en dialog der svaret er en
overføring (`transmission`) med `relatedTransmissionId` lik vår
`forsendelseId` — `Acceptance` («Innsending av godkjent
Aksjonærregisteroppgave for organisasjonsnummer …») eller etter
Dialogportens vokabular `Rejection` — med tilbakemeldingen som PDF og
XML. `POST …/submissions/{id}/outcome` henter og lagrer det
(hendelse `outcome`, én per innsending); statusen blir `godkjent`,
`avvist` eller `behandlet`. Portalen har en knapp for det, og lenker til
tilbakemeldingsdokumentene (de åpnes med innlogging hos Skatteetaten).

Alle innsendinger for samme selskap og år havner i **samme dialog**, hver
med sin egen `forsendelseId`; en ny oppgave for et år som alt er levert
ble godkjent i test uten innvendinger. Hvordan den forholder seg til den
forrige (erstatter eller legges til) er ikke bekreftet, og derfor nekter
regnmed fortsatt en andre innsending for et levert år.

### Det som sendes, lagres før det sendes

Samme regel som a-meldingen (docs/lonn.md): migrasjon 0076 lagrer
hovedskjemaet, hvert underskjema og **nøkkelen hvert kall alltid sendes
med** før første HTTP-kall. Alt er innsettings-bart; status beregnes av
hendelsesloggen (`receipt` / `failed` / `outcome`) og lagres aldri.

- **Ingen kvittering, samme innhold** (et kall feilet, nettet falt): neste
  innsending sender *det lagrede* på nytt, kall for kall, med de samme
  nøklene. Skatteetaten svarer da med sitt første svar i stedet for å
  åpne en oppgave til.
- **Ingen kvittering, endret aksjeeierbok:** den gamle står som historikk
  og en ny lagres med nye nøkler. Den gamle nådde aldri `bekreft`, så
  Skatteetaten har ikke tatt den.
- **Kvittering finnes:** nektes, se over.
- **Uverifiserte transaksjonskoder** stopper innsendingen med hindringene
  navngitt, akkurat som forhåndsvisningen.

Kvitteringen (`dialogId`, `forsendelseId`, referansen) er beviset på
levering og står på innsendingen for alltid.

### Ikke bygget ennå

- **Kundevilkårene i kraft** (#185). Innsendingen nekter til selskapet har
  godtatt dem (docs/kundevilkar.md, «Aksept»), men teksten er fortsatt et
  utkast som venter på advokat.
- **Korrigering** av en levert oppgave.
- **Svaret automatisk** (Altinn Events, slik a-meldingen har det) — i dag
  hentes det med knappen.

## Endepunkter

| Metode | Sti | Merknad |
| --- | --- | --- |
| GET | `/companies/{id}/shareholders?dato=` | aksjeeierboken per dato |
| POST | `/companies/{id}/shareholders` | registrer aksjonær |
| PUT | `/companies/{id}/shareholders/{sid}/contact` | kontaktopplysninger |
| GET | `/companies/{id}/shareholders/transaction-types` | typer + `leverbar` |
| GET/POST | `/companies/{id}/share-events` | hendelser |
| GET/POST | `/companies/{id}/dividends` | utbyttevedtak |
| GET | `/companies/{id}/reports/aksjonaeroppgave?year=&format=` | oppgaven |
| GET | `/companies/{id}/reports/aksjonaeroppgave/submissions?year=` | innsendinger m/ status og kvittering |
| POST | `/companies/{id}/reports/aksjonaeroppgave/submissions?year=` | send oppgaven til Skatteetaten (`AKSJEBOK_SKRIV`) |
| POST | `/companies/{id}/reports/aksjonaeroppgave/submissions/{sid}/outcome` | hent Skatteetatens svar fra Dialogporten |

Lesing er åpen for alle tilgangsnivåer — revisor skal kunne lese
aksjeeierboken, og §4-5 gir enhver innsynsrett. Å føre hendelser krever
`bokforing` eller `admin`.

## Bevisst utenfor v1

- **Flere aksjeklasser.** Oppgaven leveres per aksjeklasse; vi leverer
  én, med `AksjeType` `01` (ordinære aksjer). Å modellere klasser
  berører hver eneste hendelse og er en egen sak.
- **Opsjoner.**
- **Tilbakebetaling av innbetalt kapital, aksjonærlån, kildeskatt**
  (post 22 og 27) — feltene finnes i XSD-en, men reglene er egne
  kapitler.
- **Aksjonær-ID-tildeling.** Aksjonærregisteret tildeler `UTLxxxxxxxxx`;
  vi lagrer den, vi finner den ikke opp.

## Testet

- `regnmed-core::nin` — kontrollsiffer, D-/H-/syntetisk nummer,
  århundreregelen, og at et ugyldig individnummerintervall gir *ingen*
  dato.
- `regnmed-core::aksjebok` — foldingen, at retningen ligger i typen, at
  utgående i fjor er inngående i år, og at **bare verifiserte koder
  finnes** (testen teller dem).
- `regnmed-core::shareholder_return` — begge skjemaene mot Skatteetatens
  XSD-er, året uten transaksjoner, selskaps- og utenlandsk aksjonær,
  navnekutting, determinisme, og at en uverifisert kode nektes.
- `regnmed-gov::shareholder_return` — kallrekkefølgen (hovedskjema,
  hvert underskjema, bekreft) med hver sin nøkkel, og at et avvist
  underskjema stopper FØR `bekreft`.
- `regnmed-api/tests/grupper/share_register.rs` —
  `the_return_is_filed_once_and_retried_with_the_same_keys`: et nytt
  forsøk er de samme kallene med de samme nøklene, en endret
  aksjeeierbok gir en ny innsending uten gjenbrukte nøkler, og et levert
  år sendes ikke igjen. En registrert aksjonær uten aksjer i året får
  ikke underskjema.
- `regnmed-api/tests/grupper/share_register.rs` — hele veien: beregnet eierandel per
  dato, at fødselsnummeret ikke er i listingen men *er* i innsendingen,
  at ingen kan avhende flere aksjer enn de eier, at utbyttet får bilag,
  at hendelser og identitet ikke kan endres eller slettes (databasen
  selv nekter), og at oppgaven for stiftelsesåret validerer.
