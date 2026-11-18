---
title: "Payroll, time and expenses"
description: "Employees, payroll runs and the a-melding, tax cards, timesheets and expenses."
editUrl: false
sidebar:
  order: 6
---

Employees, payroll runs and the a-melding, tax cards, timesheets and expenses.

The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles](/en/utviklere/tilgang-og-roller/)); "public" means no token, "—" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.

| Method | Path | Right |
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

## Further reading (Norwegian)

- [lonn](/teknisk/lonn/)
- [timer](/teknisk/timer/)
- [utlegg](/teknisk/utlegg/)
