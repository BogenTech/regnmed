---
title: Getting started with integrations
description: From client registration to your first call against a company, with curl.
sidebar:
  order: 2
---

This walkthrough takes a machine (a web shop, a point-of-sale system, a script) from nothing to a successful call against a company's general ledger. It uses `client_credentials`, which is the only way a robot identifies itself. regnmed never issues its own API keys.

## 1. Get a client in regnid

The integration must be a **confidential client** in regnid with the `client_credentials` grant and the right `audience`. The grant is off by default per client and is given explicitly:

```sh
regnid add-client --client-id nettbutikk --name 'Nettbutikken' \
    --grant-type client_credentials --confidential --audience regnmed
```

The secret is printed once, and regnid stores only the hash.

:::caution[`--audience` must be included]
Without the right `audience`, the client logs in fine, fetches a token, and then gets `401` on every single call, with no trace anywhere that points at the client registration. The value must be the one the API requires (`OIDC_AUDIENCE`).
:::

On hosted regnmed.no, operations runs `regnid add-client`. Agents can instead be created directly in the portal (Brukere og roller, "Users and roles"), which creates identity and access in one step, see [MCP and agents](/en/developers/mcp-and-agents/). There is no self-service form for arbitrary machine clients today.

## 2. Give the client access to the company

A client without a grant gets nothing, however valid the token. An administrator in the company grants access in the portal (Administrasjon, Oppdrag og integrasjoner, "Administration, Engagements and integrations") or with the API:

```sh
curl -X POST https://api.regnmed.no/companies/$COMPANY/integrations \
  -H "Authorization: Bearer $ADMIN_TOKEN" -H 'Content-Type: application/json' \
  -d '{"client_id": "nettbutikk", "navn": "Nettbutikken", "kontakt": "it@butikk.example", "access": "bokforing"}'
```

(`navn` is the display name, `kontakt` the contact address.)

```json
{ "integration_id": "0192f0c4-…" }
```

`access` is a role name: `ansatt` (employee), `les` (read), `bokforing` (bookkeeping), `admin`, or one of the company's own roles. A name that does not exist gives `400` with the list of valid ones. The call requires the right `INTEGRASJON_ADMIN`, which a machine can never hold itself.

## 3. Get a token

The token endpoint is discovered from the issuer (`https://id.regnmed.no/.well-known/openid-configuration`). Client credentials go in the form body (`client_secret_post`):

```sh
TOKEN_URL=$(curl -s https://id.regnmed.no/.well-known/openid-configuration | jq -r .token_endpoint)

TOKEN=$(curl -s "$TOKEN_URL" \
  -d grant_type=client_credentials \
  -d client_id=nettbutikk \
  -d client_secret="$CLIENT_SECRET" | jq -r .access_token)
```

Cache the token until shortly before it expires, and fetch a new one when it has expired. The lifetime is decided by regnid, not by regnmed.

## 4. Call `/me`

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

(`plattform` is the platform role, `nye_tilganger` the number of newly granted accesses, `orgnr` the organization number, `abonnement` the subscription and `rettigheter` the rights.)

`/me` answers `200` with `companies: []` for a client that is identified but has not been given access. That is the expected answer before step 2. `rettigheter` lists what the role actually gives you, with the rights a machine never gets already filtered out.

## 5. Call a company

```sh
COMPANY=0192e8aa-…
curl -s "https://api.regnmed.no/companies/$COMPANY/reports/saldobalanse?from=2026-01-01&to=2026-12-31" \
  -H "Authorization: Bearer $TOKEN" | jq '.accounts[0]'
```

(`saldobalanse` is the trial balance.)

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

(`inngaende_ore` is the opening balance, `debet_ore` the debit total, `kredit_ore` the credit total and `utgaende_ore` the closing balance.)

First things to check when debugging:

| Response | Means |
| --- | --- |
| `401` | The token is missing, invalid or expired, or has the wrong `iss` or `aud` |
| `404` | The company does not exist, **or** the client has no access to it |
| `403` | You have access, but the role lacks the right. The error text is the name of the right |
| `429` | You have used up the per-minute budget (default 120 calls per minute) |

Next: [Conventions](/en/developers/conventions/) and [Examples](/en/developers/examples/).
