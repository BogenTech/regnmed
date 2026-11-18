---
title: Self-hosting
description: Run regnmed yourself. What you need, the environment variables that matter, and what the AGPL requires of you.
sidebar:
  order: 10
---

regnmed is open source (AGPL-3.0) and can run on your own infrastructure. Customers of the hosted service at regnmed.no need no licence from us and can skip this page.

:::caution[Self-hosting means you carry the responsibility]
The public-sector integrations (Maskinporten, Altinn, the Norwegian Tax Administration's APIs) require the organisation's own agreements and keys. They do not come with the code. The annual regulatory review (rates, code lists, schemas) is not yours either unless you do it yourself.
:::

## What you need

| Component | Requirement | Note |
| --- | --- | --- |
| **PostgreSQL** | **18 or newer**, never lower | The only state. Migrations are run by `regnmed migrate` |
| **Identity provider** | Any OIDC issuer. regnmed validates tokens and does not know the IdP | regnmed's own is **regnid**, a separate repo that is never vendored here |
| **NATS JetStream** | Optional | Outbound and inbound email share one mail rail. Without `NATS_URL`, email dispatch and inbound email are off, and say so clearly instead of pretending |
| S3-compatible storage | Optional | Documents (attachments, inbox) otherwise live in the database. All six `BLOB_S3_*` or none |
| TSA (RFC 3161) | Optional | `ANCHOR_TSA_URL` gives a third-party timestamp on the anchoring |
| OCR sidecar | Optional | `regnmed-ocr` gives text suggestions from receipt photos |

The resource budget is part of the product: `regnmed-api` uses around 13 MB RSS under load against a cap of 64 MB, enforced in CI ([frugality](/teknisk/frugality/)).

## Images

Released images are at `ghcr.io/bogentech/regnmed` with tags like `v0.13.6` and `sha-<short>`. One runtime image (distroless, static musl binaries) contains:

- `regnmed-api`, the API, which also serves the portal (the entry point)
- `regnmed`, the admin CLI (`migrate`, `verify-ledger`, `anchor` and others), used by migration and cron jobs
- `regnmed-agent`, the worker for hosted agents
- `regnmed-ocr` with models

Build for the right architecture: the image deliberately has no default `TARGET`, since a wrong architecture only fails at container start. Use `scripts/build-images.sh`.

## Locally on one machine

```sh
docker compose up -d            # Postgres 18 on port 5433 (or scripts/dev-db.sh without Docker)
cp .env.example .env
cargo run -p regnmed-cli -- migrate
cargo run -p regnmed-api
```

`.env.example` is the full reference for every environment variable the code reads, with dummy values. It must never contain a real secret.

## The variables that matter

| Variable | Meaning |
| --- | --- |
| `DATABASE_URL` | **Required.** Postgres connection |
| `OIDC_ISSUER` | **Required.** The issuer, discovered via `/.well-known/openid-configuration` |
| `OIDC_AUDIENCE` | Which `aud` is accepted. Without it the audience is not checked |
| `OIDC_JWKS_FILE` | Development and tests only: load the JWKS from a file |
| `BIND_ADDR` | Default `127.0.0.1:8080`, that is loopback. A container must set it explicitly |
| `PORTAL_BASE_URL` | The link in the invitation email. Never taken from the `Host` header |
| `PORTAL_OIDC_CLIENT_ID` | The portal's client. Default `regnmed-portal` |
| `NATS_URL`, `NATS_USER`, `NATS_PASSWORD` | Mail rail. Without them email is off |
| `MAIL_IN_DOMAIN` | The domain inbound voucher addresses are shown under. Set it only once MX actually delivers |
| `ANCHOR_TSA_URL` | RFC 3161 witness for the anchoring |
| `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`, `REGNMED_DRIFT_ORGNR` | The card rail for subscriptions. Both Stripe variables or neither. If you are not a service that charges money, you do not need them |
| `AGENT_SEALING_KEY` | Required for hosted agents (the customer's LLM key is stored sealed) |
| `MASKINPORTEN_*` | Public-sector integrations. All required ones together or none |

In Kubernetes (`deploy/`, kustomize with overlays `local`, `test`, `prod`) the secrets come from Kubernetes secrets created outside git. **No secrets in the repo, in any form**, not even encrypted.

## After the first rollout

The manifests cannot create the state inside regnid. Two steps are done once per environment:

```sh
regnid add-client --client-id regnmed-portal --name "regnmed portal" \
    --redirect-uri https://din.host/callback \
    --post-logout-redirect-uri https://din.host/ \
    --audience regnmed
```

`--audience` is the one that bites if forgotten: sign-in works, and then every API call answers `401`. The first account is created with `regnid add-user`. A company administrator is not created by hand: whoever onboards the company from the Brønnøysund Register (Enhetsregisteret) becomes the administrator. Full setup, backup with a weekly restore test, and hardening: [Deployment](/teknisk/deploy/).

## Commonly used operations commands

```sh
regnmed migrate               # run migrations
regnmed verify-ledger         # walk all chains from genesis, check attachments and anchoring
regnmed anchor                # take an anchoring snapshot (nightly cron job)
```

## What the AGPL-3.0 means for you

- You are free to run, modify and self-host regnmed.
- If you modify it and let others use it **over a network**, you must make the complete corresponding source available to them (AGPL §13). If you run it unmodified for yourself, nothing obliges you.
- Customers of the hosted service need no licence. A separate commercial licence is available by agreement for the unusual case where the AGPL does not fit, but that is not how regnmed is sold. The product is the service.

See [LICENSE](https://github.com/BogenTech/regnmed/blob/main/LICENSE) and [COMMERCIAL-LICENSE.md](https://github.com/BogenTech/regnmed/blob/main/COMMERCIAL-LICENSE.md).
