---
title: Autentisering
description: regnmed er en OIDC relying party. Slik valideres tokenet, hva `aud` gjør, og hva tokenet aldri bærer.
sidebar:
  order: 3
---

regnmed er **bare** en OIDC relying party. Identitetstilbyderen er regnid (`https://id.regnmed.no`), og regnmed validerer tokens mot en konfigurert issuer og dens JWKS. Ingen IdP-spesifikke detaljer er bakt inn: enhver OIDC-kompatibel issuer fungerer, og det er det selvhosting bygger på.

## To veier, ett token

| Hvem | Flyt | Merknad |
| --- | --- | --- |
| Menneske (portalen) | Authorization code + PKCE mot regnid | Portalens klient heter `regnmed-portal`. Tokenutvekslingen proxies via `/auth/token` |
| Maskin eller agent | `client_credentials` mot regnid | Konfidensiell klient, `sub` = `client_id` |

Resten av API-et ser ikke forskjell: begge kommer som `Authorization: Bearer <token>`. Portalens `/auth/*`-stier hører til portalen og er ikke en del av integrasjons-API-et.

## Hva som valideres

API-et avviser tokenet med `401` med mindre alt dette stemmer:

- **Algoritmen er asymmetrisk**: `RS256`, `RS384`, `RS512`, `ES256`, `ES384` eller `EdDSA`. `HS*` avvises, ellers kunne alle med den offentlige JWKS forfalske tokens.
- **Headeren har en `kid`** som finnes i issuerens JWKS. Ukjent `kid` utløser én ny henting av JWKS (nøkkelrotasjon), og avvises så.
- **`iss`** er identisk med issuer-verdien i discovery-dokumentet.
- **`aud`** inneholder den konfigurerte audience, når API-et krever en (`OIDC_AUDIENCE`). Hostede regnmed.no krever `regnmed`.
- **`exp`** er ikke passert. Levetiden settes av regnid, ikke av regnmed.

Feilmeldingene er bevisst få og sier lite: `missing Authorization header`, `expected a Bearer token` og `invalid token`. Årsaken ved et avvist token står bare i serverloggen.

:::caution[`aud` er den vanligste feilen]
En klient uten riktig `audience` registrert i regnid får et gyldig token som API-et avviser. Innloggingen ser ut til å virke, og hvert kall gir `401`. Sjekk `aud` i tokenet (det er en vanlig JWT) før du leter andre steder.
:::

## Hva tokenet bærer

regnmed leser tre påstander: `sub`, `name` og `email`. Det er alt. **Ingen roller, ingen selskaper, ingen rettigheter** ligger i tokenet. En regnskapsfører med 60 klienter kunne ikke båret det i en JWT, og tilgang som kan trekkes tilbake samme dag må ligge der den kan endres samme dag.

`sub` slås opp i regnmeds base. Er den en registrert integrasjon, brukes integrasjonens registrerte navn, og **tokenet kan ikke gi roboten nytt navn** i revisjonssporet. Er den ukjent, opprettes personen første gang den ses (just-in-time).

## Tilgang kommer fra databasen

Etter identiteten avgjør regnmed hva den får gjøre, per selskap og per kall. Se [Tilgang og roller](/developers/access-and-roles/). `GET /me` er stedet å spørre: det forteller hvilke selskaper identiteten når, i hvilken rolle, og med hvilke rettigheter.

## Test mot en lokal instans

For utvikling kan API-et starte uten en levende IdP ved å laste JWKS fra fil:

```sh
OIDC_ISSUER=https://id.example.invalid \
OIDC_JWKS_FILE=.dev/jwks.json \
OIDC_AUDIENCE=regnmed \
cargo run -p regnmed-api
```

Signaturene valideres fullt ut også da. Testene i repoet signerer ekte RS256-tokens mot en generert JWKS. `OIDC_JWKS_FILE` er kun for utvikling og tester.
