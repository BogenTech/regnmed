---
title: Access and roles for integrations
description: How a robot gets access, what it can never get, and how a human remains the second pair of eyes.
sidebar:
  order: 4
---

The sentence is the same for humans and machines: **the token proves identity, regnmed decides what the identity may do.** That is why there is no separate machine route in. An integration is technically a `person` with `kind = 'integrasjon'`, and access lookup, attribution and audit trail are identical to a human's.

## One role per company

An administrator gives the integration a **role name** in the company, the same way as for a member:

| Role | Can |
| --- | --- |
| `ansatt` (employee) | Own hours and own expenses. Not the general ledger |
| `les` (read) | Everything an auditor can read: reports, subledger, vouchers. Not payroll |
| `bokforing` (bookkeeping) | In addition: post, invoice, upload to the inbox |
| `admin` | In addition what runs the company, including **migration** |
| custom | What the company itself has composed from the rights vocabulary |

`revisor` (auditor) cannot be assigned to a machine, for the same reason it cannot be assigned directly to a human: it follows an audit engagement.

Access is modelled as an engagement. `valid_to` is **exclusive**, so a revocation takes effect at that instant, not at midnight. The row stays, recording who granted and who revoked.

## The ceiling: a machine never widens access

Whatever the role, a machine **never** gets a right that governs who has access:

| Denied to a machine | What it is |
| --- | --- |
| `MEDLEM_ADMIN` | Members, invitations, roles |
| `OPPDRAG_ADMIN` | Engagements for firms |
| `INTEGRASJON_ADMIN` | Other integrations' access, and its own |
| `SELSKAP_ADMIN` | Company details, registration status |

The filter lives in **the access lookup itself** (`Right::can_be_delegated`), not at assignment. That is why it also applies to roles added later, and to rows written before the rule existed. In short: a robot does the job, but never widens access. Even if you give it `admin`, these four stay out of reach, and `/me` does not promise them in `rettigheter` (rights).

## 404 and 403

- **`404`** means the company does not exist **or** the identity has no access to it. You must not be able to learn that a company exists.
- **`403`** means you have access, but not enough. The error text is the name of the missing right, for example `{"error": "BILAG_BOKFOR"}`, so an administrator can see what must be granted.
- An **expired subscription** gives `403` on everything that changes data (`abonnementet er utløpt …`, "the subscription has expired …"), but never on reading, export or running the company. The general ledger is not held hostage.

## `/me` tells you what you have

`GET /me` returns, per company, `access` (the role name), `via` (`direkte` (direct), the firm name for an engagement, or `integrasjon`) and `rettigheter`, the list of slugs you actually hold. Use it to switch features off instead of trying and getting `403`. The list is display only. The server's guard decides every call.

The rights vocabulary (72 slugs such as `FAKTURA_SKRIV` and `LONN_KJOR`) and the roles are described in [Access and roles (technical)](/teknisk/auth/), with a machine-checked matrix.

## Approval: a human as the second pair of eyes

The company can require that a machine never posts alone. When `attestation_policy.krev_for_maskin` is set, **everything** an integration posts requires approval (`attestering`) by a **human**, regardless of the amount limit, and an agent cannot approve another agent's voucher. Manual posting (`POST …/vouchers`) is refused under an active policy above the limit, with a pointer to the inbox, so the side door does not undo the control.

See [Approval](/teknisk/attestering/).

## Attribution and log

- Everything an integration posts gets `created_by` = the integration's registered name. Not "system", not a uuid.
- Every **mutating** request is logged (method, path, status), and a counter per integration, company and day covers all calls, including reads. Both are visible in the portal to everyone with access to the company.
- A `client_id` that belongs to a human with access cannot be registered as an integration. A robot never inherits a human's access.

Endpoints for managing integrations: see [Integrations, agents and messages](/en/developers/api/agents/). Full background is in [Machine access to the API](/teknisk/integrations/).
