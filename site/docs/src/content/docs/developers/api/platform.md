---
title: "Plattform (leverandørens konsoll)"
description: "Krever aktiv plattformrolle (`support` leser og tildeler nye medlemskap, `systemadmin` endrer); hvert kall logges, og ingen av endepunktene når en hovedbok. Ikke en del av integrasjons-API-et."
editUrl: false
sidebar:
  order: 11
---

Krever aktiv plattformrolle (`support` leser og tildeler nye medlemskap, `systemadmin` endrer); hvert kall logges, og ingen av endepunktene når en hovedbok. Ikke en del av integrasjons-API-et.

Tabellen er generert fra rutetabellen i `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Rett** er rettigheten handleren krever (se [Tilgang og roller](/developers/access-and-roles/)); «åpen» betyr uten token, «—» at kontrollen ligger i en hjelpefunksjon eller at bare innlogging kreves. Der flere retter står, avhenger kravet av parametrene.

| Metode | Sti | Rett |
| --- | --- | --- |
| GET | `/platform/companies` | plattformrolle |
| GET | `/platform/companies/{company_id}` | plattformrolle |
| DELETE | `/platform/companies/{company_id}/members/{person_id}` | plattformrolle |
| POST | `/platform/companies/{company_id}/members/{person_id}/restore` | plattformrolle |
| PUT | `/platform/companies/{company_id}/settings` | plattformrolle |
| POST | `/platform/companies/{company_id}/subscription` | plattformrolle |
| POST | `/platform/companies/{company_id}/subscription/end` | plattformrolle |
| GET | `/platform/customers` | plattformrolle |
| GET | `/platform/firms` | plattformrolle |
| GET | `/platform/members` | plattformrolle |
| POST | `/platform/members` | plattformrolle |
| DELETE | `/platform/members/{member_id}` | plattformrolle |
| GET | `/platform/overview` | plattformrolle |
| GET | `/platform/plans` | plattformrolle |
| PUT | `/platform/plans/{slug}` | plattformrolle |
| POST | `/platform/plans/{slug}/prices` | plattformrolle |
| GET | `/platform/settings` | plattformrolle |
| PUT | `/platform/settings` | plattformrolle |
| GET | `/platform/subscriptions` | plattformrolle |
| GET | `/platform/users` | plattformrolle |
| POST | `/platform/users/{person_id}/companies/{company_id}` | plattformrolle |
| POST | `/platform/users/{person_id}/firms/{firm_id}` | plattformrolle |

## Les mer

- [auth](/teknisk/auth/)
