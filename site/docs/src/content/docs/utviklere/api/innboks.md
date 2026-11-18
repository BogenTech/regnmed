---
title: "Innboks, import og jobber"
description: "Bilagsinnboksen, forslag, e-post-inn, SAF-T- og CSV-import og bakgrunnsjobber."
sidebar:
  order: 7
---

Bilagsinnboksen, forslag, e-post-inn, SAF-T- og CSV-import og bakgrunnsjobber.

Tabellen er generert fra rutetabellen i `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Rett** er rettigheten handleren krever (se [Tilgang og roller](/utviklere/tilgang-og-roller/)); «åpen» betyr uten token, «—» at kontrollen ligger i en hjelpefunksjon eller at bare innlogging kreves. Der flere retter står, avhenger kravet av parametrene.

| Metode | Sti | Rett |
| --- | --- | --- |
| POST | `/companies/{company_id}/import/contacts` | `MIGRERING_ADMIN` |
| POST | `/companies/{company_id}/import/open-items` | `MIGRERING_ADMIN` |
| POST | `/companies/{company_id}/import/saft` | `MIGRERING_ADMIN` |
| POST | `/companies/{company_id}/import/saft/analyze` | `MIGRERING_ADMIN` |
| GET | `/companies/{company_id}/import/saft/log` | `BILAG_LES` |
| GET | `/companies/{company_id}/inbox` | `BILAG_LES` |
| POST | `/companies/{company_id}/inbox` | `BILAG_LAST_OPP` |
| GET | `/companies/{company_id}/inbox/mail` | `EPOST_INN_LES` |
| POST | `/companies/{company_id}/inbox/mail/{mail_id}/reject` | `EPOST_INN_ADMIN` |
| POST | `/companies/{company_id}/inbox/mail/{mail_id}/release` | `EPOST_INN_ADMIN` |
| GET | `/companies/{company_id}/inbox/settings` | `EPOST_INN_LES` |
| POST | `/companies/{company_id}/inbox/settings/address` | `EPOST_INN_ADMIN` |
| POST | `/companies/{company_id}/inbox/settings/senders` | `EPOST_INN_ADMIN` |
| DELETE | `/companies/{company_id}/inbox/settings/senders/{sender_id}` | `EPOST_INN_ADMIN` |
| POST | `/companies/{company_id}/inbox/{document_id}/attester` | `ATTESTERING_UTFOR` |
| GET | `/companies/{company_id}/inbox/{document_id}/attestering` | `ATTESTERING_LES` |
| POST | `/companies/{company_id}/inbox/{document_id}/avvis` | `BILAG_BOKFOR` |
| POST | `/companies/{company_id}/inbox/{document_id}/bokfor` | `BILAG_BOKFOR` |
| GET | `/companies/{company_id}/inbox/{document_id}/content` | `BILAG_LES` |
| GET | `/companies/{company_id}/inbox/{document_id}/ehf` | `BILAG_LES` |
| POST | `/companies/{company_id}/inbox/{document_id}/expense` | `BILAG_BOKFOR` |
| GET | `/companies/{company_id}/inbox/{document_id}/forslag` | `BILAG_LES` |
| POST | `/companies/{company_id}/jobs/audit-report` | — |
| POST | `/companies/{company_id}/jobs/saft-import` | — |
| GET | `/companies/{company_id}/jobs/{job_id}` | — |
| POST | `/companies/{company_id}/opening-balance` | `MIGRERING_ADMIN` |

## Opplasting

`POST /companies/{id}/inbox?filename=<navn>&sha256=<hex>` med filen som rå kropp og riktig `Content-Type`. `sha256` er valgfri, men gjør et nytt forsøk trygt. Svar: `{ "document_id": "…", "existing": false }`. Se [Konvensjoner](/utviklere/konvensjoner/#idempotens).


## Les mer

- [bilagsinnboks](/teknisk/bilagsinnboks/)
- [bilagstolkning](/teknisk/bilagstolkning/)
- [epost-inn](/teknisk/epost-inn/)
- [migration](/teknisk/migration/)
- [jobs](/teknisk/jobs/)
