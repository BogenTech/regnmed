---
title: "Regnskap og rapporter"
description: "Lovpålagte spesifikasjoner, resultat og balanse, mva og mva-melding, SAF-T, nøkkeltall, budsjett og revisjonsrapporten."
editUrl: false
sidebar:
  order: 4
---

Lovpålagte spesifikasjoner, resultat og balanse, mva og mva-melding, SAF-T, nøkkeltall, budsjett og revisjonsrapporten.

Tabellen er generert fra rutetabellen i `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Rett** er rettigheten handleren krever (se [Tilgang og roller](/utviklere/tilgang-og-roller/)); «åpen» betyr uten token, «—» at kontrollen ligger i en hjelpefunksjon eller at bare innlogging kreves. Der flere retter står, avhenger kravet av parametrene.

| Metode | Sti | Rett |
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

## Les mer

- [rapporter](/teknisk/rapporter/)
- [mva](/teknisk/mva/)
- [budsjett](/teknisk/budsjett/)
- [revisjon](/teknisk/revisjon/)
- [SAF-T](https://github.com/BogenTech/regnmed/blob/main/docs/saft/README.md)
