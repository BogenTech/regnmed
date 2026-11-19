---
title: "Platform (the vendor console)"
description: "Requires an active platform role (`support` reads and grants new memberships, `systemadmin` changes). Every call is logged and none of the endpoints reach a ledger. Not part of the integration API."
editUrl: false
sidebar:
  order: 11
---

Requires an active platform role (`support` reads and grants new memberships, `systemadmin` changes). Every call is logged and none of the endpoints reach a ledger. Not part of the integration API.

The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles](/en/developers/access-and-roles/)); "public" means no token, "—" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.

| Method | Path | Right |
| --- | --- | --- |
| GET | `/platform/companies` | platform role |
| GET | `/platform/companies/{company_id}` | platform role |
| POST | `/platform/companies/{company_id}/addons` | platform role |
| POST | `/platform/companies/{company_id}/addons/{addon}/end` | platform role |
| DELETE | `/platform/companies/{company_id}/members/{person_id}` | platform role |
| POST | `/platform/companies/{company_id}/members/{person_id}/restore` | platform role |
| PUT | `/platform/companies/{company_id}/settings` | platform role |
| POST | `/platform/companies/{company_id}/subscription` | platform role |
| POST | `/platform/companies/{company_id}/subscription/end` | platform role |
| GET | `/platform/customers` | platform role |
| GET | `/platform/firms` | platform role |
| GET | `/platform/members` | platform role |
| POST | `/platform/members` | platform role |
| DELETE | `/platform/members/{member_id}` | platform role |
| GET | `/platform/overview` | platform role |
| GET | `/platform/plans` | platform role |
| PUT | `/platform/plans/{slug}` | platform role |
| POST | `/platform/plans/{slug}/prices` | platform role |
| GET | `/platform/settings` | platform role |
| PUT | `/platform/settings` | platform role |
| GET | `/platform/subscriptions` | platform role |
| GET | `/platform/users` | platform role |
| POST | `/platform/users/{person_id}/companies/{company_id}` | platform role |
| POST | `/platform/users/{person_id}/firms/{firm_id}` | platform role |

## Further reading (Norwegian)

- [auth](/teknisk/auth/)
