---
title: API reference
description: Overview of all endpoints, grouped by area, with the right each endpoint requires.
sidebar:
  order: 0
---

The reference is split into areas. Each page lists method, path and the right the handler requires, generated from the route table in the code. Request and response shapes for the most used calls are in [Examples](/en/utviklere/eksempler/). Each area points to the technical document that explains the rules behind it (in Norwegian).

| Area | Scope |
| --- | --- |
| [Identity and access](/en/utviklere/api/identitet/) | `/me`, members, roles, invitations, firm and engagement, subscription, terms |
| [Company and general ledger](/en/utviklere/api/hovedbok/) | Onboarding, chart of accounts, vouchers, attachments, period lock, dimensions, year-end close |
| [Sales and subledger](/en/utviklere/api/salg/) | Invoices, quotes, orders, products, customers, open items |
| [Accounting and reports](/en/utviklere/api/rapporter/) | Specifications, income statement, balance sheet, VAT, SAF-T, audit, budget |
| [Bank and payments](/en/utviklere/api/bank/) | Reconciliation, OCR, payment lists, cards, currency |
| [Payroll, time and expenses](/en/utviklere/api/personal/) | Employees, payroll, a-melding, time, expenses |
| [Inbox, import and jobs](/en/utviklere/api/innboks/) | Voucher inbox, inbound e-mail, SAF-T and CSV import |
| [Share register and assets](/en/utviklere/api/aksjer/) | Shareholders, dividends, RF-1086, asset register |
| [Integrations, agents and messages](/en/utviklere/api/agenter/) | Machine access, agent tasks, conversations |
| [Anchoring and verification](/en/utviklere/api/verifisering/) | Public feed, per-company proofs |
| [Platform](/en/utviklere/api/plattform/) | The vendor console (not an integration API) |

## Common rules

- Authentication: `Authorization: Bearer <token>`. See [Authentication](/en/utviklere/autentisering/).
- All company endpoints live under `/companies/{company_id}/…`. Without access the response is `404`.
- Amounts in øre, dates `YYYY-MM-DD`, errors as `{"error": "…"}`. See [Conventions](/en/utviklere/konvensjoner/).
- Endpoints with separate rights for "own" and "all" (`_EGNE` and `_ALLE`): `_ALLE` implies `_EGNE`.

## Outside the reference

| Path | Why |
| --- | --- |
| `/`, `/callback`, `/assets/*`, `/ny`, `/portal-config`, `/manifest.webmanifest`, `/sw.js`, icons | Serve the web portal |
| `/auth/token`, `/auth/refresh`, `/auth/logout`, `/auth/device`, `/auth/device/poll` | The portal's sign-in, not for integrations |
| `/webhooks/stripe`, `/webhooks/altinn` | Inbound calls from Stripe and Altinn, signed by them. Not for you to call |

The longer, hand-written overview in the repository ([docs/api.md](https://github.com/BogenTech/regnmed/blob/main/docs/api.md)) lags behind the route table somewhat; the reference here is generated from the code.
