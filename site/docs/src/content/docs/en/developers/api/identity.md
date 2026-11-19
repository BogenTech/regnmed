---
title: "Identity and access"
description: "Who am I and who may do what: `/me`, members, roles, invitations, firm, engagement, subscription and the public terms pages."
editUrl: false
sidebar:
  order: 1
---

Who am I and who may do what: `/me`, members, roles, invitations, firm, engagement, subscription and the public terms pages.

The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles](/en/developers/access-and-roles/)); "public" means no token, "—" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.

| Method | Path | Right |
| --- | --- | --- |
| GET | `/companies/{company_id}/access` | `MEDLEM_ADMIN` |
| GET | `/companies/{company_id}/access/history` | `MEDLEM_ADMIN` |
| DELETE | `/companies/{company_id}/access/{person_id}` | `MEDLEM_ADMIN` |
| PUT | `/companies/{company_id}/access/{person_id}` | `MEDLEM_ADMIN` |
| POST | `/companies/{company_id}/access/{person_id}/restore` | `MEDLEM_ADMIN` |
| GET | `/companies/{company_id}/attestering/policy` | `ATTESTERING_LES` |
| POST | `/companies/{company_id}/attestering/policy` | `ATTESTERING_ADMIN` |
| GET | `/companies/{company_id}/authority-calls` | `SELSKAP_LES` |
| GET | `/companies/{company_id}/engagement-requests` | `OPPDRAG_LES` |
| POST | `/companies/{company_id}/engagement-requests` | `OPPDRAG_ADMIN` |
| POST | `/companies/{company_id}/engagement-requests/{request_id}/withdraw` | `OPPDRAG_ADMIN` |
| GET | `/companies/{company_id}/engagements` | `OPPDRAG_LES` |
| POST | `/companies/{company_id}/engagements/{engagement_id}/end` | `OPPDRAG_ADMIN` |
| GET | `/companies/{company_id}/invitations` | `MEDLEM_ADMIN` |
| POST | `/companies/{company_id}/invitations` | `MEDLEM_ADMIN`, `LONN_SKRIV` |
| DELETE | `/companies/{company_id}/invitations/{invitation_id}` | `MEDLEM_ADMIN` |
| POST | `/companies/{company_id}/invitations/{invitation_id}/resend` | `MEDLEM_ADMIN` |
| GET | `/companies/{company_id}/members` | `ATTESTERING_ADMIN` |
| GET | `/companies/{company_id}/platform-access` | `MEDLEM_ADMIN` |
| GET | `/companies/{company_id}/roles` | `MEDLEM_ADMIN`, `ALLE` |
| POST | `/companies/{company_id}/roles` | `MEDLEM_ADMIN` |
| GET | `/companies/{company_id}/roles/history` | `MEDLEM_ADMIN` |
| PUT | `/companies/{company_id}/roles/{role_id}` | `MEDLEM_ADMIN` |
| POST | `/companies/{company_id}/roles/{role_id}/deactivate` | `MEDLEM_ADMIN` |
| POST | `/companies/{company_id}/roles/{role_id}/restore` | `MEDLEM_ADMIN` |
| GET | `/companies/{company_id}/subscription` | `SELSKAP_LES` |
| POST | `/companies/{company_id}/subscription` | `SELSKAP_ADMIN` |
| POST | `/companies/{company_id}/subscription/cancel` | `SELSKAP_ADMIN` |
| POST | `/companies/{company_id}/subscription/card-setup` | `SELSKAP_ADMIN` |
| POST | `/companies/{company_id}/subscription/change` | `SELSKAP_ADMIN` |
| GET | `/companies/{company_id}/terms` | `SELSKAP_LES` |
| POST | `/companies/{company_id}/terms/accept` | `SELSKAP_ADMIN` |
| GET | `/directory/firms` | — |
| POST | `/firms` | — |
| GET | `/firms/mine` | — |
| GET | `/firms/{firm_id}/access` | — |
| GET | `/firms/{firm_id}/access/history` | — |
| DELETE | `/firms/{firm_id}/access/{person_id}` | — |
| PUT | `/firms/{firm_id}/access/{person_id}` | — |
| POST | `/firms/{firm_id}/access/{person_id}/restore` | — |
| GET | `/firms/{firm_id}/clients` | — |
| GET | `/firms/{firm_id}/contacts` | — |
| PUT | `/firms/{firm_id}/engagements/{engagement_id}/contacts` | — |
| GET | `/firms/{firm_id}/invitations` | — |
| POST | `/firms/{firm_id}/invitations` | — |
| DELETE | `/firms/{firm_id}/invitations/{invitation_id}` | — |
| POST | `/firms/{firm_id}/invitations/{invitation_id}/resend` | — |
| GET | `/firms/{firm_id}/platform-access` | — |
| GET | `/firms/{firm_id}/requests` | — |
| POST | `/firms/{firm_id}/requests/{request_id}/decision` | — |
| GET | `/health` | public |
| GET | `/me` | — |
| GET | `/pricing` | public |
| GET | `/registry/enheter/{orgnumber}` | — |
| GET | `/terms` | public |
| GET | `/terms/versions` | public |
| GET | `/vilkar` | public |
| GET | `/vilkar/{version}` | public |

## Further reading (Norwegian)

- [auth](/teknisk/auth/)
- [integrations](/teknisk/integrations/)
- [marketplace](/teknisk/marketplace/)
- [abonnement](/teknisk/abonnement/)
- [vilkar-teknisk](/teknisk/vilkar-teknisk/)
