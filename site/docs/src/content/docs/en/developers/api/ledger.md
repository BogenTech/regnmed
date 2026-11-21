---
title: "Company and general ledger"
description: "Company onboarding, chart of accounts, vouchers, attachments, period lock, dimensions, accruals, year-end close, balance documentation and cash-register settlement."
editUrl: false
sidebar:
  order: 2
---

Company onboarding, chart of accounts, vouchers, attachments, period lock, dimensions, accruals, year-end close, balance documentation and cash-register settlement.

The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles](/en/developers/access-and-roles/)); "public" means no token, "—" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.

| Method | Path | Right |
| --- | --- | --- |
| POST | `/companies` | — |
| GET | `/companies/{company_id}/accounts` | `BILAG_LES` |
| POST | `/companies/{company_id}/accounts` | `BILAG_BOKFOR` |
| PUT | `/companies/{company_id}/accounts/{account_number}` | `BILAG_BOKFOR` |
| PUT | `/companies/{company_id}/accounts/{account_number}/reskontro` | `RESKONTRO_SKRIV` |
| GET | `/companies/{company_id}/altinn/system-user` | `SELSKAP_ADMIN` |
| POST | `/companies/{company_id}/altinn/system-user` | `SELSKAP_ADMIN` |
| POST | `/companies/{company_id}/altinn/system-user/check` | `SELSKAP_ADMIN` |
| GET | `/companies/{company_id}/arsavslutning` | `RAPPORT_LES` |
| POST | `/companies/{company_id}/arsavslutning` | `BILAG_BOKFOR`, `PERIODE_LAAS` |
| GET | `/companies/{company_id}/arsavslutning/{year}/forslag` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/attachments/{attachment_id}` | `BILAG_LES` |
| GET | `/companies/{company_id}/balansedokumentasjon` | `RAPPORT_LES` |
| POST | `/companies/{company_id}/balansedokumentasjon` | `BILAG_BOKFOR` |
| GET | `/companies/{company_id}/balansedokumentasjon/historikk` | `RAPPORT_LES` |
| POST | `/companies/{company_id}/balansedokumentasjon/vedlegg` | `BILAG_BOKFOR` |
| GET | `/companies/{company_id}/balansedokumentasjon/{id}/vedlegg` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/dimensions` | `DIMENSJON_LES` |
| POST | `/companies/{company_id}/dimensions` | `DIMENSJON_SKRIV` |
| GET | `/companies/{company_id}/dimensions/prosjekt/{code}/satser` | `TIMER_SATS_SKRIV` |
| POST | `/companies/{company_id}/dimensions/prosjekt/{code}/satser` | `TIMER_SATS_SKRIV` |
| PUT | `/companies/{company_id}/dimensions/{kind}/{code}` | `DIMENSJON_SKRIV` |
| POST | `/companies/{company_id}/kassaoppgjor` | `BILAG_BOKFOR` |
| POST | `/companies/{company_id}/kassaoppgjor/rapport` | `BILAG_BOKFOR` |
| GET | `/companies/{company_id}/peppol/sender` | `SELSKAP_ADMIN` |
| POST | `/companies/{company_id}/peppol/sender` | `SELSKAP_ADMIN` |
| GET | `/companies/{company_id}/period-lock` | `BILAG_LES` |
| PUT | `/companies/{company_id}/period-lock` | `PERIODE_LAAS` |
| GET | `/companies/{company_id}/periodiseringer` | `BILAG_LES` |
| POST | `/companies/{company_id}/periodiseringer` | `BILAG_BOKFOR` |
| POST | `/companies/{company_id}/periodiseringer/{id}/kjor` | `BILAG_BOKFOR` |
| GET | `/companies/{company_id}/periodiseringer/{id}/kjoringer` | `BILAG_LES` |
| POST | `/companies/{company_id}/periodiseringer/{id}/stopp` | `BILAG_BOKFOR` |
| GET | `/companies/{company_id}/settings` | `SELSKAP_LES` |
| PUT | `/companies/{company_id}/settings` | `SELSKAP_ADMIN` |
| POST | `/companies/{company_id}/settings/registrering` | `SELSKAP_ADMIN` |
| GET | `/companies/{company_id}/vouchers` | `BILAG_LES` |
| POST | `/companies/{company_id}/vouchers` | `BILAG_BOKFOR` |
| GET | `/companies/{company_id}/vouchers/{voucher_id}` | `BILAG_LES` |
| GET | `/companies/{company_id}/vouchers/{voucher_id}/attachments` | `BILAG_LES` |
| POST | `/companies/{company_id}/vouchers/{voucher_id}/attachments` | `VEDLEGG_SKRIV` |
| POST | `/companies/{company_id}/vouchers/{voucher_id}/reverse` | `BILAG_BOKFOR` |
| GET | `/notifications` | — |
| GET | `/web` | — |

## Further reading (Norwegian)

- [hovedbok](/teknisk/hovedbok/)
- [ledger](/teknisk/ledger/)
- [perioder](/teknisk/perioder/)
- [dimensjoner](/teknisk/dimensjoner/)
- [periodisering](/teknisk/periodisering/)
- [arsavslutning](/teknisk/arsavslutning/)
- [balansedokumentasjon](/teknisk/balansedokumentasjon/)
- [kontantsalg](/teknisk/kontantsalg/)
