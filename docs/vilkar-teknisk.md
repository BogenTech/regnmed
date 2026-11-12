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
skattekortbestilling — nekter til gjeldende versjon er godtatt.

**Endres vilkårsfila, endres versjonen**, og hvert selskap må godta på
nytt før neste innsending. Det gjelder også skrivefeil — det er prisen
for at «godtatt» alltid betyr én bestemt tekst. Derfor står ingenting
som beskriver koden i selve vilkårene.

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
