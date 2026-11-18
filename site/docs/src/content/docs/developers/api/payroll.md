---
title: "Lønn, timer og utlegg"
description: "Ansatte, lønnskjøring og a-melding, skattekort, timeføring og utlegg."
editUrl: false
sidebar:
  order: 6
---

Ansatte, lønnskjøring og a-melding, skattekort, timeføring og utlegg.

Tabellen er generert fra rutetabellen i `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Rett** er rettigheten handleren krever (se [Tilgang og roller](/developers/access-and-roles/)); «åpen» betyr uten token, «—» at kontrollen ligger i en hjelpefunksjon eller at bare innlogging kreves. Der flere retter står, avhenger kravet av parametrene.

| Metode | Sti | Rett |
| --- | --- | --- |
| GET | `/companies/{company_id}/employees` | `LONN_LES` |
| POST | `/companies/{company_id}/employees` | `LONN_SKRIV` |
| PUT | `/companies/{company_id}/employees/{employee_id}/employment` | `LONN_SKRIV` |
| PUT | `/companies/{company_id}/employees/{employee_id}/end` | `LONN_SKRIV` |
| DELETE | `/companies/{company_id}/employees/{employee_id}/link` | `LONN_SKRIV` |
| POST | `/companies/{company_id}/employees/{employee_id}/link` | `LONN_SKRIV` |
| GET | `/companies/{company_id}/employees/{employee_id}/link/history` | `LONN_LES` |
| GET | `/companies/{company_id}/expenses` | — |
| POST | `/companies/{company_id}/expenses` | — |
| GET | `/companies/{company_id}/expenses/addresses` | `UTLEGG_SKRIV_EGNE`, `RESKONTRO_LES` |
| GET | `/companies/{company_id}/expenses/billable` | `UTLEGG_LES_ALLE` |
| GET | `/companies/{company_id}/expenses/card-lines` | `UTLEGG_SKRIV_EGNE`, `UTLEGG_GODKJENN` |
| GET | `/companies/{company_id}/expenses/claimants` | `UTLEGG_GODKJENN` |
| GET | `/companies/{company_id}/expenses/kjoring` | `UTLEGG_SKRIV_EGNE` |
| POST | `/companies/{company_id}/expenses/kjoring` | `UTLEGG_SKRIV_EGNE` |
| POST | `/companies/{company_id}/expenses/receipts` | `UTLEGG_SKRIV_EGNE` |
| GET | `/companies/{company_id}/expenses/reise` | `UTLEGG_SKRIV_EGNE` |
| GET | `/companies/{company_id}/expenses/reise/days` | `UTLEGG_SKRIV_EGNE` |
| POST | `/companies/{company_id}/expenses/reise/preview` | `UTLEGG_SKRIV_EGNE` |
| GET | `/companies/{company_id}/expenses/route` | `UTLEGG_LES_EGNE` |
| POST | `/companies/{company_id}/expenses/utlegg` | `UTLEGG_SKRIV_EGNE` |
| GET | `/companies/{company_id}/expenses/{expense_id}` | — |
| PUT | `/companies/{company_id}/expenses/{expense_id}` | — |
| POST | `/companies/{company_id}/expenses/{expense_id}/approve` | `UTLEGG_GODKJENN` |
| POST | `/companies/{company_id}/expenses/{expense_id}/discard` | — |
| POST | `/companies/{company_id}/expenses/{expense_id}/pay` | `UTLEGG_UTBETAL` |
| GET | `/companies/{company_id}/expenses/{expense_id}/receipt` | — |
| PUT | `/companies/{company_id}/expenses/{expense_id}/receipt` | — |
| POST | `/companies/{company_id}/expenses/{expense_id}/reject` | `UTLEGG_GODKJENN` |
| POST | `/companies/{company_id}/expenses/{expense_id}/submit` | — |
| GET | `/companies/{company_id}/expenses/{expense_id}/suggestion` | — |
| GET | `/companies/{company_id}/payroll` | `LONN_LES` |
| POST | `/companies/{company_id}/payroll` | `LONN_KJOR` |
| GET | `/companies/{company_id}/payroll/hours/{employee_id}` | `LONN_LES` |
| GET | `/companies/{company_id}/payroll/months/{period}/amelding` | `LONN_KJOR` |
| GET | `/companies/{company_id}/payroll/months/{period}/amelding/submissions` | — |
| POST | `/companies/{company_id}/payroll/months/{period}/amelding/submissions` | — |
| POST | `/companies/{company_id}/payroll/months/{period}/amelding/submissions/{submission_id}/feedback` | — |
| GET | `/companies/{company_id}/payroll/{run_id}/amelding` | `LONN_KJOR` |
| GET | `/companies/{company_id}/payroll/{run_id}/amelding/submissions` | — |
| POST | `/companies/{company_id}/payroll/{run_id}/amelding/submissions` | — |
| POST | `/companies/{company_id}/payroll/{run_id}/amelding/submissions/{submission_id}/feedback` | — |
| GET | `/companies/{company_id}/payroll/{run_id}/slip/{employee_id}` | `LONNSSLIPP_LES_EGEN`, `LONNSSLIPP_LES_ALLE` |
| GET | `/companies/{company_id}/tax-cards` | `LONN_LES` |
| POST | `/companies/{company_id}/tax-cards` | `LONN_SKRIV` |
| POST | `/companies/{company_id}/tax-cards/{order_id}/answer` | `LONN_SKRIV` |
| GET | `/companies/{company_id}/timesheet` | `TIMER_LES_EGNE`, `TIMER_LES_ALLE` |
| POST | `/companies/{company_id}/timesheet` | `TIMER_SATS_SKRIV` |
| POST | `/companies/{company_id}/timesheet/invoice` | `TIMER_FAKTURER` |
| GET | `/companies/{company_id}/timesheet/lock` | `TIMER_LES_EGNE` |
| PUT | `/companies/{company_id}/timesheet/lock` | `TIMER_LAAS` |
| GET | `/companies/{company_id}/timesheet/summary` | `TIMER_RAPPORT_LES` |
| GET | `/companies/{company_id}/timesheet/unbilled` | `TIMER_RAPPORT_LES` |
| DELETE | `/companies/{company_id}/timesheet/{entry_id}` | `TIMER_SKRIV_ALLE` |
| PUT | `/companies/{company_id}/timesheet/{entry_id}` | `TIMER_SKRIV_ALLE`, `TIMER_SATS_SKRIV` |

## Les mer

- [lonn](/teknisk/lonn/)
- [timer](/teknisk/timer/)
- [utlegg](/teknisk/utlegg/)
