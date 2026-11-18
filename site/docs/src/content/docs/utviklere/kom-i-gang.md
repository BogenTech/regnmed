---
title: Kom i gang for integrasjoner
description: Fra klientregistrering til første kall mot et selskap, med curl.
sidebar:
  order: 2
---

Denne veiviseren tar en maskin (en nettbutikk, et kassasystem, et script) fra ingenting til et vellykket kall mot et selskaps hovedbok. Den bruker `client_credentials`, som er den eneste måten en robot identifiserer seg på. regnmed utsteder aldri egne API-nøkler.

## 1. Få en klient i regnid

Integrasjonen må være en **konfidensiell klient** i regnid med grantet `client_credentials` og riktig `audience`. Grantet er av som standard per klient og gis uttrykkelig:

```sh
regnid add-client --client-id nettbutikk --name 'Nettbutikken' \
    --grant-type client_credentials --confidential --audience regnmed
```

Hemmeligheten skrives ut én gang, og regnid lagrer bare hashen.

:::caution[`--audience` må være med]
Uten riktig `audience` logger klienten seg fint inn, henter token, og får så `401` på hvert eneste kall, uten spor noe sted som peker på klientregistreringen. Verdien må være den API-et krever (`OIDC_AUDIENCE`).
:::

På hostede regnmed.no er det driften som kjører `regnid add-client`. Agenter kan i stedet opprettes direkte i portalen (Brukere og roller), som lager identitet og tilgang i ett steg, se [MCP og agenter](/utviklere/mcp-og-agenter/). Det finnes ikke noe selvbetjent skjema for vilkårlige maskinklienter i dag.

## 2. Gi klienten tilgang til selskapet

En klient uten tildeling får ingenting, uansett hvor gyldig tokenet er. En administrator i selskapet gir tilgang i portalen (Administrasjon, Oppdrag og integrasjoner) eller med API-et:

```sh
curl -X POST https://api.regnmed.no/companies/$COMPANY/integrations \
  -H "Authorization: Bearer $ADMIN_TOKEN" -H 'Content-Type: application/json' \
  -d '{"client_id": "nettbutikk", "navn": "Nettbutikken", "kontakt": "it@butikk.example", "access": "bokforing"}'
```

```json
{ "integration_id": "0192f0c4-…" }
```

`access` er et rollenavn: `ansatt`, `les`, `bokforing`, `admin` eller en av selskapets egne roller. Et navn som ikke finnes gir `400` med listen over gyldige. Kallet krever rettigheten `INTEGRASJON_ADMIN`, som en maskin selv aldri kan ha.

## 3. Hent et token

Token-endepunktet oppdages fra issueren (`https://id.regnmed.no/.well-known/openid-configuration`). Klientlegitimasjonen sendes i skjemakroppen (`client_secret_post`):

```sh
TOKEN_URL=$(curl -s https://id.regnmed.no/.well-known/openid-configuration | jq -r .token_endpoint)

TOKEN=$(curl -s "$TOKEN_URL" \
  -d grant_type=client_credentials \
  -d client_id=nettbutikk \
  -d client_secret="$CLIENT_SECRET" | jq -r .access_token)
```

Cache tokenet til like før det utløper, og hent et nytt når det er gått ut. Levetiden bestemmes av regnid, ikke av regnmed.

## 4. Kall `/me`

```sh
curl -s https://api.regnmed.no/me -H "Authorization: Bearer $TOKEN" | jq
```

```json
{
  "person_id": "0192f0c4-…",
  "sub": "nettbutikk",
  "name": "Nettbutikken",
  "email": null,
  "plattform": null,
  "nye_tilganger": 0,
  "companies": [
    {
      "company_id": "0192e8aa-…",
      "orgnr": "999888777",
      "name": "Butikk AS",
      "access": "bokforing",
      "via": "integrasjon",
      "abonnement": { "status": "aktiv", "dato": null },
      "rettigheter": ["BILAG_LES", "BILAG_BOKFOR", "FAKTURA_SKRIV", "…"]
    }
  ]
}
```

`/me` svarer `200` med `companies: []` for en klient som er identifisert men ikke har fått tilgang. Det er det forventede svaret før steg 2. `rettigheter` lister hva rollen faktisk gir deg, med de rettighetene en maskin aldri får allerede filtrert bort.

## 5. Kall et selskap

```sh
COMPANY=0192e8aa-…
curl -s "https://api.regnmed.no/companies/$COMPANY/reports/saldobalanse?from=2026-01-01&to=2026-12-31" \
  -H "Authorization: Bearer $TOKEN" | jq '.accounts[0]'
```

```json
{
  "number": "1920",
  "name": "Bankinnskudd",
  "inngaende_ore": 0,
  "debet_ore": 12500000,
  "kredit_ore": 4200000,
  "utgaende_ore": 8300000
}
```

Feil først i feilsøkingen:

| Svar | Betyr |
| --- | --- |
| `401` | Tokenet mangler, er ugyldig, utløpt, har feil `iss` eller feil `aud` |
| `404` | Selskapet finnes ikke, **eller** klienten har ingen tilgang til det |
| `403` | Du har tilgang, men rollen mangler rettigheten. Feilteksten er rettighetens navn |
| `429` | Du har brukt opp minuttbudsjettet (standard 120 kall per minutt) |

Neste steg: [Konvensjoner](/utviklere/konvensjoner/) og [Eksempler](/utviklere/eksempler/).
