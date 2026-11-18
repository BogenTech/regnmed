---
title: Authentication
description: regnmed is an OIDC relying party. How the token is validated, what `aud` does, and what the token never carries.
sidebar:
  order: 3
---

regnmed is **only** an OIDC relying party. The identity provider is regnid (`https://id.regnmed.no`), and regnmed validates tokens against a configured issuer and its JWKS. No IdP-specific details are baked in: any OIDC-compliant issuer works, and self-hosting builds on that.

## Two routes, one token

| Who | Flow | Note |
| --- | --- | --- |
| Human (the portal) | Authorization code + PKCE against regnid | The portal's client is called `regnmed-portal`. The token exchange is proxied through `/auth/token` |
| Machine or agent | `client_credentials` against regnid | Confidential client, `sub` = `client_id` |

The rest of the API sees no difference: both arrive as `Authorization: Bearer <token>`. The portal's `/auth/*` paths belong to the portal and are not part of the integration API.

## What is validated

The API rejects the token with `401` unless all of this holds:

- **The algorithm is asymmetric**: `RS256`, `RS384`, `RS512`, `ES256`, `ES384` or `EdDSA`. `HS*` is rejected, otherwise anyone with the public JWKS could forge tokens.
- **The header has a `kid`** that exists in the issuer's JWKS. An unknown `kid` triggers one fresh fetch of the JWKS (key rotation), and is then rejected.
- **`iss`** is identical to the issuer value in the discovery document.
- **`aud`** contains the configured audience, when the API requires one (`OIDC_AUDIENCE`). Hosted regnmed.no requires `regnmed`.
- **`exp`** has not passed. The lifetime is set by regnid, not by regnmed.

The error messages are deliberately few and say little: `missing Authorization header`, `expected a Bearer token` and `invalid token`. The reason for a rejected token appears only in the server log.

:::caution[`aud` is the most common mistake]
A client without the right `audience` registered in regnid gets a valid token that the API rejects. Login appears to work, and every call gives `401`. Check `aud` in the token (it is an ordinary JWT) before looking anywhere else.
:::

## What the token carries

regnmed reads three claims: `sub`, `name` and `email`. That is all. **No roles, no companies, no rights** are in the token. An accountant with 60 clients could not carry that in a JWT, and access that can be revoked the same day must live where it can be changed the same day.

`sub` is looked up in regnmed's database. If it is a registered integration, the integration's registered name is used, and **the token cannot rename the robot** in the audit trail. If it is unknown, the person is created the first time it is seen (just-in-time).

## Access comes from the database

After identity, regnmed decides what it may do, per company and per call. See [Access and roles](/en/developers/access-and-roles/). `GET /me` is the place to ask: it tells you which companies the identity reaches, in which role, and with which rights.

## Test against a local instance

For development the API can start without a live IdP by loading the JWKS from a file:

```sh
OIDC_ISSUER=https://id.example.invalid \
OIDC_JWKS_FILE=.dev/jwks.json \
OIDC_AUDIENCE=regnmed \
cargo run -p regnmed-api
```

Signatures are fully validated then too. The tests in the repo sign real RS256 tokens against a generated JWKS. `OIDC_JWKS_FILE` is for development and tests only.
