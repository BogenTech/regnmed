---
title: "Inbox, import and jobs"
description: "The voucher inbox, suggestions, inbound e-mail, SAF-T and CSV import and background jobs."
editUrl: false
sidebar:
  order: 7
---

The voucher inbox, suggestions, inbound e-mail, SAF-T and CSV import and background jobs.

The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles](/en/utviklere/tilgang-og-roller/)); "public" means no token, "—" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.

| Method | Path | Right |
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

## Upload

`POST /companies/{id}/inbox?filename=<name>&sha256=<hex>` with the file as the raw body and the right `Content-Type`. `sha256` is optional but makes a retry safe. Response: `{ "document_id": "…", "existing": false }`. See [Conventions](/en/utviklere/konvensjoner/#idempotens).


## Further reading (Norwegian)

- [bilagsinnboks](/teknisk/bilagsinnboks/)
- [bilagstolkning](/teknisk/bilagstolkning/)
- [epost-inn](/teknisk/epost-inn/)
- [migration](/teknisk/migration/)
- [jobs](/teknisk/jobs/)
