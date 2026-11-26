# Kundevilkårene i koden

Hvordan regnmed håndhever kundevilkårene (`docs/kundevilkar.md`). Denne
fila er dokumentasjon, **ikke** en del av vilkårene: vilkårsteksten er
hashet og må godtas på nytt hver gang den endres, så det som beskriver
koden står her og kan rettes uten at hver kunde må godta noe.

## Aksept (§1.4)

Teksten i `docs/kundevilkar.md` **er** vilkårene: binæren serverer den
uendret på `/vilkar` (ren tekst) og `/terms` (JSON), og versjonen er
SHA-256 av teksten. En administrator (`SELSKAP_ADMIN`, som aldri kan gis
til en integrasjon) godtar den under Administrasjon → Firmaopplysninger;
aksepten lagres innsettings-bart i `terms_acceptance` (migrasjon 0077)
med selskap, person, navn, tidspunkt og hashen. Hvert kall til
myndighetene på kundens vegne — a-melding, aksjonærregisteroppgave,
skattekortbestilling — nekter til versjonen som gjelder i dag (eller en
nyere) er godtatt.

**Endres vilkårsfila, endres versjonen**, og hvert selskap må godta på
nytt. Det gjelder også skrivefeil — det er prisen for at «godtatt»
alltid betyr én bestemt tekst. Derfor står ingenting som beskriver koden
eller gjennomgangen av den i selve vilkårene: sporingstabellen og
spørsmålene til advokaten står i `docs/kundevilkar-gjennomgang.md`.

## Varsling av nye vilkår (§14.1)

En versjon gjelder fra datoen i arkivfilnavnet, ikke fra den dagen den
rulles ut. Vilkårene lover minst 30 dagers varsel, så en ny tekst
arkiveres med en dato minst 30 dager fram i tid. Fram til den datoen:

- gjelder versjonen før, og et selskap som har godtatt den, kan sende
  inn som før;
- viser portalen den nye teksten og datoen den gjelder fra, og en
  administrator kan godta den med en gang;
- `GET /companies/{id}/terms` svarer `kan_sende_inn: true` og
  `godtatt: false`, med `gjelder_fra` satt til den nye datoen.

Fra datoen nekter innsendingen for selskaper som ikke har godtatt den nye
versjonen. Endringer Digdir, Skatteetaten eller regelverket krever, kan
dateres tidligere (§14.1). En versjon datert i dag eller tidligere
binder fra utrullingen.

Testen `an_announced_version_binds_only_from_its_date` fester regelen.
`the_current_text_is_archived` krever at vilkårsfila har den nyeste
datoen i arkivet, og at to versjoner ikke gjelder fra samme dato
(utover de tre utkastene fra 2026-11-12, som er eldre enn regelen).

### E-postvarselet

CronJobben `regnmed terms-notice` (`deploy/shared/terms-notice.yaml`,
daglig) sender e-post til hvert selskap som har godtatt en tidligere
versjon, men verken har godtatt eller fått varsel om den gjeldende.
Selskaper som aldri har godtatt vilkårene, er ikke bundet av dem og får
ikke varsel. Mottakeren er selskapets egen e-postadresse (§15.1), ellers
adressen til den som sist godtok vilkårene for selskapet. E-posten sier
når den nye versjonen gjelder fra, lenker til den
(`PORTAL_BASE_URL/vilkar/<sha256>`) og forteller hva som skjer om den
ikke godtas.

Hvert varsel lagres innsettings-bart i `terms_notice` (migrasjon 0109):
selskap, versjon, mottaker og tidspunkt, én rad per selskap og versjon.
Derfor får hvert selskap varselet én gang, og radens id er e-postens
Nats-Msg-Id. Et selskap uten noen adresse rapporteres, og jobben feiler
til noen har lagt inn en adresse; da sendes varselet ved neste kjøring.

Varselet går ut første kjøring etter utrullingen. **30 dagers varsel
krever derfor at den nye versjonen dateres minst 30 dager etter
utrullingsdagen**, ikke etter dagen teksten ble skrevet.

Testen `each_bound_company_is_told_once` (`regnmed-mail`) kjører mot en
ekte nats-server og fester hvem som varsles og at det skjer én gang.

## Versjoner

Hver versjon som har gjeldt, arkiveres i `docs/kundevilkar/` som
`<dato>-<sha256>.md` (datoen den trer i kraft) og serveres på
`/vilkar/<sha256>`; `/terms/versions` lister dem med gjeldende versjon
først. En kunde som godtok en eldre tekst kan dermed alltid lese nettopp
den, og selskapets egen aksepthistorikk står i
`GET /companies/{id}/terms`. Testen `the_current_text_is_archived`
feiler hvis vilkårene endres uten at den nye teksten arkiveres, hvis en
arkivert tekst redigeres i etterkant, og hvis en arkivert versjon
**slettes** — listen over kjente versjoner kan bare vokse.

## Registeret over kall til myndighetene (§7.1)

`GET /companies/{id}/authority-calls` viser hvert kall regnmed har gjort
til Altinn og myndighetene på selskapets vegne: skjema, hva det gjaldt,
hvem som ba om det, når, og hva som kom tilbake. Det er ikke en egen
logg, men en lesing av sporene kallene selv etterlater, og alle er
innsettings-bare:

| Kall | Spor | Skrives |
| --- | --- | --- |
| A-melding | `amelding_submission` + `_event` (0074) | **før** kallet; kvittering, feil og tilbakemelding etterpå |
| Aksjonærregisteroppgave | `shareholder_return_submission` + `_event` (0076) | **før** kallet; kvittering, feil og svar etterpå |
| Skattekortbestilling | `tax_card_request` (0079) + `tax_card_order`, `tax_card_request_failure`, `tax_card` | **før** kallet; mottatt bestilling, feil og svar etterpå |
| Abonnement på Altinn-hendelser | `altinn_event_subscription` (0075) | **etter** kallet, når Altinn har opprettet det — et mislykket forsøk logges bare i driftsloggen |

Registeret sier **at** et kall feilet, aldri hvorfor. Feilteksten er
Skatteetatens svar og kan bære lønns- eller aksjonæropplysninger, mens
registeret kan leses med `SELSKAP_LES` alene. Teksten står bak hvert
skjemas egen rettighet (`LONN_LES`, `AKSJEBOK_LES`).

## Tilgangsgjennomgangen (§4.2)

Kundens plikt; regnmed håndhever tilgangene (§4.1), men gjennomfører
ikke gjennomgangen for kunden.
