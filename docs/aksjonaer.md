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
`regnmed-core::fnr`. Nummeret leses ett sted — når oppgaven bygges — og
integrasjonstesten fester det: aksjeeierbok-svaret skal ikke inneholde
nummeret, innsendingen skal.

`regnmed-core::fnr` validerer også kontrollsifrene (MOD11, to runder) og
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

### Den ærlige begrensningen: transaksjonstypekodene

Dette er den ene tingen #43 ikke kan love, og den er verdt å lese nøye.

Hver bevegelse skal rapporteres med en **transaksjonstypekode**
(`AksjeErvervType`, `AksjerArvMvOmsattType`,
`AksjerNyutstedteStiftelseMvType`). Vi har verifisert at:

- ingen av feltene er begrenset i XSD-en — alle tre er ubundet
  `Tekst35`, uten `enumeration`;
- rettledningen RF-1087 **navngir** transaksjonstypene (post 23 tilgang,
  post 24 omfordeling, post 25 avgang) men oppgir ingen koder;
- kodelistene finnes som et eget artefakt distribuert til
  sluttbrukersystemer — Skatteetatens egne SBS-nyheter omtaler
  «kodelister ... for RF-1086» som noe som kan endres uavhengig av
  skjemaet.

Vi har altså de autoritative **navnene** (og bruker dem), men ikke
kodene. Den eneste koden vi kan belegge er `N` for
stiftelse/nyemisjon, fra etatens eget publiserte eksempel.

**Derfor gjetter vi ikke.** `Transaksjonstype::kode()` returnerer `Some`
bare for verifiserte koder, og rendringen **nekter høylytt** for resten:

```
transaksjonstypen «salg» kan ikke leveres: RF-1086-koden for den er ikke
publisert i XSD-en eller rettledningen, og regnmed gjetter den ikke
```

Begrunnelsen er ikke pedanteri. Transaksjonstypen flyter videre inn i
aksjonærens **egen** aksjeoppgave (RF-1088) og bestemmer inngangsverdi
og skjermingsgrunnlag. En høylytt nektelse er noe man kan rette; en
stille feilrapportert transaksjon er det ikke.

Praktisk betydning:

- Et år **uten** eierbevegelser — det vanligste for et lite AS — rendres
  fullt ut, med aksjonærer og utbytte.
- **Stiftelsesåret** rendres, fordi `N` er verifisert.
- Et år med **salg, arv, gave, fusjon, splitt** o.l. stopper, og sier
  hvilken type som mangler kode.

Forhåndsvisningen (`format` utelatt) **dør ikke** av dette: den viser
tallene og lister hindringene under `leverbar: false`, slik at brukeren
ser både regnskapet og hva som stanser leveringen. Bare `format=xml`
feiler.

**Slik lukkes gapet:** når Maskinporten-scopet
`skatteetaten:innrapporteringaksjonaerregisteroppgave` er tildelt, kan
kodene verifiseres mot testmiljøet og legges inn i `kode()` med kilde —
ett sted, med en test som teller hvor mange koder som er verifiserte.

## Innsending (ikke bygget, og hvorfor)

API-et hos Skatteetaten har fem endepunkter: POST hovedskjema → POST
underskjema (én per aksjonær) → POST bekreft, pluss GET dokument(er).
`idempotencyKey` er påkrevd per kall.

Vi implementerer **ikke** innsendingen nå, av samme grunn som
mva-meldingens innsending venter (docs/gov.md, #8): den krever

- Maskinporten-scope `skatteetaten:innrapporteringaksjonaerregisteroppgave`
  (må bestilles hos Skatteetaten), og
- **Altinn systembruker** med tilgangspakke (f.eks.
  `regnskapsforer-med-signeringsrettighet`) — en nyere
  Digdir-mekanisme vi ikke har satt opp.

Å skrive en klient mot et API vi ikke kan kjøre ville vært gjetting i
enda et lag. Rendringen er den halvparten vi kan stå inne for, og
XML-filene kan lastes ned og leveres gjennom et system som har
tilgangene.

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

- `regnmed-core::fnr` — kontrollsiffer, D-/H-/syntetisk nummer,
  århundreregelen, og at et ugyldig individnummerintervall gir *ingen*
  dato.
- `regnmed-core::aksjebok` — foldingen, at retningen ligger i typen, at
  utgående i fjor er inngående i år, og at **bare verifiserte koder
  finnes** (testen teller dem).
- `regnmed-core::aksjonaeroppgave` — begge skjemaene mot Skatteetatens
  XSD-er, året uten transaksjoner, selskaps- og utenlandsk aksjonær,
  navnekutting, determinisme, og at en uverifisert kode nektes.
- `regnmed-api/tests/grupper/aksjonaer.rs` — hele veien: beregnet eierandel per
  dato, at fødselsnummeret ikke er i listingen men *er* i innsendingen,
  at ingen kan avhende flere aksjer enn de eier, at utbyttet får bilag,
  at hendelser og identitet ikke kan endres eller slettes (databasen
  selv nekter), og at oppgaven for stiftelsesåret validerer.
