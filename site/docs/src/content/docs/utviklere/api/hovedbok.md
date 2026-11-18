---
title: "Selskap og hovedbok"
description: "Onboarding av selskap, kontoplan, bilag, vedlegg, periodelås, dimensjoner, periodisering, årsavslutning, balansedokumentasjon og kassaoppgjør."
sidebar:
  order: 2
---

Onboarding av selskap, kontoplan, bilag, vedlegg, periodelås, dimensjoner, periodisering, årsavslutning, balansedokumentasjon og kassaoppgjør.

Tabellen er generert fra rutetabellen i `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Rett** er rettigheten handleren krever (se [Tilgang og roller](/utviklere/tilgang-og-roller/)); «åpen» betyr uten token, «—» at kontrollen ligger i en hjelpefunksjon eller at bare innlogging kreves. Der flere retter står, avhenger kravet av parametrene.

| Metode | Sti | Rett |
| --- | --- | --- |
| POST | `/companies` | — |
| GET | `/companies/{company_id}/accounts` | `BILAG_LES` |
| POST | `/companies/{company_id}/accounts` | `BILAG_BOKFOR` |
| PUT | `/companies/{company_id}/accounts/{account_number}` | `BILAG_BOKFOR` |
| PUT | `/companies/{company_id}/accounts/{account_number}/reskontro` | `RESKONTRO_SKRIV` |
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

## Les mer

- [hovedbok](/teknisk/hovedbok/)
- [ledger](/teknisk/ledger/)
- [perioder](/teknisk/perioder/)
- [dimensjoner](/teknisk/dimensjoner/)
- [periodisering](/teknisk/periodisering/)
- [arsavslutning](/teknisk/arsavslutning/)
- [balansedokumentasjon](/teknisk/balansedokumentasjon/)
- [kontantsalg](/teknisk/kontantsalg/)
