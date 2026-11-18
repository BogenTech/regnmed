---
title: Tilgang og roller for integrasjoner
description: Hvordan en robot får tilgang, hva den aldri kan få, og hvordan et menneske forblir andre øye.
sidebar:
  order: 4
---

Setningen er den samme for mennesker og maskiner: **tokenet beviser identitet, regnmed avgjør hva identiteten får gjøre.** Derfor finnes det ingen egen maskinvei inn. En integrasjon er teknisk en `person` med `kind = 'integrasjon'`, og tilgangsoppslag, attribusjon og revisjonsspor er identiske med et menneskes.

## En rolle per selskap

En administrator gir integrasjonen et **rollenavn** i selskapet, på samme måte som for et medlem:

| Rolle | Kan |
| --- | --- |
| `ansatt` | Egne timer og egne utlegg. Ikke hovedboken |
| `les` | Alt en revisor kan lese: rapporter, reskontro, bilag. Ikke lønn |
| `bokforing` | I tillegg: bokføre, fakturere, laste opp i innboksen |
| `admin` | I tillegg det som styrer selskapet, inkludert **migrering** |
| egendefinert | Det selskapet selv har satt sammen av rettighetsvokabularet |

`revisor` kan ikke tildeles en maskin, av samme grunn som den ikke kan tildeles et menneske direkte: den følger et revisjonsoppdrag.

Tilgangen er modellert som et oppdrag. `valid_to` er **eksklusiv**, så en tilbakekalling virker i samme øyeblikk, ikke ved midnatt. Raden blir stående med hvem som ga og hvem som trakk tilbake.

## Taket: en maskin utvider aldri tilgang

Uansett rolle får en maskin **aldri** en rettighet som styrer hvem som har tilgang:

| Nektet en maskin | Hva det er |
| --- | --- |
| `MEDLEM_ADMIN` | Medlemmer, invitasjoner, roller |
| `OPPDRAG_ADMIN` | Oppdrag til byråer |
| `INTEGRASJON_ADMIN` | Andre integrasjoners tilgang, og sin egen |
| `SELSKAP_ADMIN` | Firmaopplysninger, registreringsstatus |

Filteret ligger i **selve tilgangsoppslaget** (`Right::can_be_delegated`), ikke ved tildelingen. Derfor gjelder det også roller som kommer til senere, og rader skrevet før regelen fantes. Sagt kort: en robot gjør jobben, men utvider aldri tilgang. Selv om du gir den `admin`, er disse fire fortsatt utenfor rekkevidde, og `/me` lover dem ikke i `rettigheter`.

## 404 og 403

- **`404`** betyr at selskapet ikke finnes **eller** at identiteten ikke har noen tilgang til det. Du skal ikke kunne lære at et selskap finnes.
- **`403`** betyr at du har tilgang, men ikke nok. Feilteksten er navnet på rettigheten som mangler, for eksempel `{"error": "BILAG_BOKFOR"}`, slik at en administrator kan se hva som må gis.
- Et **utløpt abonnement** gir `403` på alt som endrer (`abonnementet er utløpt …`), men aldri på lesing, eksport eller styring av selskapet. Hovedboken tas ikke som gissel.

## `/me` forteller hva du har

`GET /me` returnerer per selskap `access` (rollenavnet), `via` (`direkte`, firmanavnet for et oppdrag, eller `integrasjon`) og `rettigheter`, listen med slugger du faktisk har. Bruk den til å slå av funksjoner i stedet for å prøve og få `403`. Listen er bare visning. Det er serverens vakt som avgjør hvert kall.

Rettighetsvokabularet (72 slugger som `FAKTURA_SKRIV` og `LONN_KJOR`) og rollene er beskrevet i [Tilgang og roller (teknisk)](/teknisk/auth/), med en maskinsjekket matrise.

## Attestering: et menneske som andre øye

Selskapet kan kreve at en maskin aldri bokfører alene. Når `attestation_policy.krev_for_maskin` er satt, krever **alt** en integrasjon bokfører godkjent attestering av et **menneske**, uavhengig av beløpsgrensen, og en agent kan ikke attestere en annen agents bilag. Manuell føring (`POST …/vouchers`) nektes under en aktiv policy over grensen, med en henvisning til innboksen, slik at sidedøren ikke opphever kontrollen.

Se [Attestering](/teknisk/attestering/).

## Attribusjon og logg

- Alt en integrasjon bokfører får `created_by` = integrasjonens registrerte navn. Ikke «system», ikke en uuid.
- Hver **endrende** forespørsel logges (metode, sti, status), og en teller per integrasjon, selskap og dag dekker alle kall, også lesinger. Begge er synlige i portalen for alle med tilgang til selskapet.
- En `client_id` som tilhører et menneske med tilgang kan ikke registreres som integrasjon. En robot arver aldri et menneskes tilgang.

Endepunkter for å administrere integrasjoner: se [Integrasjoner, agenter og meldinger](/developers/api/agents/). Fullt bakgrunnsstoff står i [Maskin-tilgang til API-et](/teknisk/integrations/).
