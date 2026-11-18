---
title: Introduction
description: How the regnmed API is built, what you can rely on, and where to start as a developer.
sidebar:
  order: 1
---

**The web is the product, and the API is the platform.** Everything the portal does, it does over the same API you call yourself. A human and an integration hit the same endpoints under the same rules, and neither has a back door. The command line (`regnmed`) is for operations and administration, and is never the only way to a feature.

## Addresses

| What | Address |
| --- | --- |
| API | `https://api.regnmed.no` |
| Identity provider (regnid) | `https://id.regnmed.no` |
| Portal | `https://regnmed.no` |
| Open anchoring feed | `https://api.regnmed.no/anchors` |

The portal and the API are served by the same binary, so `https://regnmed.no` also answers on the API paths. Use `api.regnmed.no` in integrations.

## The model: person, firm, engagement, company

```text
person ──member of──▶ firm (accountant / auditor)
   │                      │
   │                      └── engagement (scope + validity) ──▶ company
   └──direct member (role)──────────────────────────────────▶ company
```

(On the wire these are `person`, `firma`, `oppdrag` and `selskap`.)

- A **person** is an identity from regnid. A human and a robot are both persons.
- A **company** (`selskap`) owns a general ledger. All access goes to a company.
- A **firm** (`firma`) is an accounting or auditing practice. It gets access to companies through **engagements** (`oppdrag`: an agreement with a scope and a validity period), not through a list in the token.
- Your access is the **union of all ways in**: direct membership with a role, engagements through a firm, and, for a machine, an assigned role name.

The main rule: **the token proves identity, regnmed decides what the identity may do.** Access lives in regnmed's database, never in the token. See [Access and roles](/en/utviklere/tilgang-og-roller/).

## What you can rely on

- **Amounts are integer øre**, never floats. Positive is debit, negative is credit.
- **The general ledger is append-only.** Corrections are reversing vouchers. You can verify the chain yourself, see [Verify it yourself](/en/utviklere/verifiser-selv/).
- **No access gives `404`, not `403`.** You cannot learn that a company exists without having access to it.

## Stability

The endpoints in the [API reference](/en/utviklere/api/) are used by the portal and are considered stable. Fields may be added. Existing fields are removed or change meaning only with a notice in the CHANGELOG and a transition period. New endpoints appear with each item in the ROADMAP. Responses have no version prefix today.

**Wire names stay Norwegian.** The JSON fields are partly Norwegian, because the wire format predates the language policy (`orgnr` (organization number), `termin` (VAT period), `leveringsdato` (delivery date), `prosjekt` (project)). The examples in this documentation use the keys exactly as they appear on the wire, with a gloss where one first appears. The rest of this section is English; the user guide and the technical documentation are Norwegian.

## Where do you start?

| You want to … | Read |
| --- | --- |
| Call the API from a script or a web shop | [Getting started](/en/utviklere/kom-i-gang/) |
| Understand the token, `aud` and client registration | [Authentication](/en/utviklere/autentisering/) |
| Give a robot the right access | [Access and roles](/en/utviklere/tilgang-og-roller/) |
| See what amounts, dates and errors look like | [Conventions](/en/utviklere/konvensjoner/) |
| Copy ready-made calls | [Examples](/en/utviklere/eksempler/) |
| Look up an endpoint | [API reference](/en/utviklere/api/) |
| Give an LLM agent tools | [MCP and agents](/en/utviklere/mcp-og-agenter/) |
| Check that the general ledger has not been touched | [Verify it yourself](/en/utviklere/verifiser-selv/) |
| Run regnmed yourself | [Self-hosting](/en/utviklere/selvhosting/) |
| Contribute to the code | [Contributing](/en/utviklere/bidra/) |

Audit-facing documentation about what the system guarantees and where it is tested lives under [Technical documentation](/teknisk/).
