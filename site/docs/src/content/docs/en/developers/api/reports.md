---
title: "Accounting and reports"
description: "Statutory specifications, income statement and balance sheet, VAT and the VAT return, SAF-T, key figures, budget and the audit report."
editUrl: false
sidebar:
  order: 4
---

Statutory specifications, income statement and balance sheet, VAT and the VAT return, SAF-T, key figures, budget and the audit report.

The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles](/en/developers/access-and-roles/)); "public" means no token, "—" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.

| Method | Path | Right |
| --- | --- | --- |
| GET | `/companies/{company_id}/budgets` | `BUDSJETT_LES` |
| POST | `/companies/{company_id}/budgets` | `BUDSJETT_SKRIV` |
| DELETE | `/companies/{company_id}/budgets/{budget_id}` | `BUDSJETT_SKRIV` |
| GET | `/companies/{company_id}/budgets/{budget_id}` | `BUDSJETT_LES` |
| POST | `/companies/{company_id}/budgets/{budget_id}/fastsett` | `BUDSJETT_SKRIV` |
| PUT | `/companies/{company_id}/budgets/{budget_id}/lines` | `BUDSJETT_SKRIV` |
| GET | `/companies/{company_id}/mva/terminordning` | `RAPPORT_LES` |
| POST | `/companies/{company_id}/mva/terminordning` | `MVA_ORDNING_ADMIN` |
| GET | `/companies/{company_id}/reports/avvik` | `BUDSJETT_LES` |
| GET | `/companies/{company_id}/reports/balanse` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/reports/bokforingsspesifikasjon` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/reports/kontospesifikasjon` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/reports/kundespesifikasjon` | — |
| GET | `/companies/{company_id}/reports/leverandorspesifikasjon` | — |
| GET | `/companies/{company_id}/reports/mva` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/reports/mva-melding` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/reports/mva/postings` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/reports/nokkeltall` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/reports/prosjekt` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/reports/resultat` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/reports/revisjon` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/reports/saft` | `RAPPORT_LES` |
| GET | `/companies/{company_id}/reports/saldobalanse` | `RAPPORT_LES` |

## Further reading (Norwegian)

- [rapporter](/teknisk/rapporter/)
- [mva](/teknisk/mva/)
- [budsjett](/teknisk/budsjett/)
- [revisjon](/teknisk/revisjon/)
- [SAF-T](https://github.com/BogenTech/regnmed/blob/main/docs/saft/README.md)
