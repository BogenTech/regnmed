---
title: "Sales and subledger"
description: "Invoices, quotes and orders, products and inventory, customers and suppliers, open items and matching."
editUrl: false
sidebar:
  order: 3
---

Invoices, quotes and orders, products and inventory, customers and suppliers, open items and matching.

The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles](/en/utviklere/tilgang-og-roller/)); "public" means no token, "—" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.

| Method | Path | Right |
| --- | --- | --- |
| GET | `/companies/{company_id}/inventory` | `LAGER_LES` |
| POST | `/companies/{company_id}/inventory/count` | `LAGER_SKRIV` |
| GET | `/companies/{company_id}/inventory/movements` | `LAGER_LES` |
| POST | `/companies/{company_id}/inventory/movements` | `LAGER_SKRIV` |
| GET | `/companies/{company_id}/invoice-templates` | `FAKTURAMAL_LES` |
| POST | `/companies/{company_id}/invoice-templates` | `FAKTURAMAL_SKRIV` |
| PUT | `/companies/{company_id}/invoice-templates/{template_id}` | `FAKTURAMAL_SKRIV` |
| POST | `/companies/{company_id}/invoice-templates/{template_id}/generate` | `FAKTURAMAL_SKRIV` |
| GET | `/companies/{company_id}/invoice-templates/{template_id}/runs` | `FAKTURAMAL_LES` |
| GET | `/companies/{company_id}/invoices` | `FAKTURA_LES` |
| POST | `/companies/{company_id}/invoices` | `FAKTURA_SKRIV`, `TIMER_FAKTURER`, `UTLEGG_LES_ALLE` |
| GET | `/companies/{company_id}/invoices/overdue` | `PURRING_LES` |
| GET | `/companies/{company_id}/invoices/{invoice_id}` | `FAKTURA_LES` |
| POST | `/companies/{company_id}/invoices/{invoice_id}/credit-note` | `FAKTURA_SKRIV` |
| GET | `/companies/{company_id}/invoices/{invoice_id}/ehf` | `FAKTURA_LES` |
| POST | `/companies/{company_id}/invoices/{invoice_id}/payments` | `FAKTURA_SKRIV`, `BILAG_BOKFOR` |
| GET | `/companies/{company_id}/invoices/{invoice_id}/pdf` | `FAKTURA_LES` |
| GET | `/companies/{company_id}/invoices/{invoice_id}/reminders` | `PURRING_LES` |
| POST | `/companies/{company_id}/invoices/{invoice_id}/reminders` | `PURRING_SKRIV` |
| GET | `/companies/{company_id}/invoices/{invoice_id}/reminders/{reminder_id}` | `PURRING_LES` |
| POST | `/companies/{company_id}/invoices/{invoice_id}/reminders/{reminder_id}/send` | `FAKTURA_SEND` |
| POST | `/companies/{company_id}/invoices/{invoice_id}/send` | `FAKTURA_SEND` |
| GET | `/companies/{company_id}/invoices/{invoice_id}/utsendelser` | `FAKTURA_LES` |
| GET | `/companies/{company_id}/orders` | — |
| POST | `/companies/{company_id}/orders` | — |
| POST | `/companies/{company_id}/orders/{order_id}/invoice` | `TILBUD_SKRIV` |
| GET | `/companies/{company_id}/orders/{order_id}/pdf` | `TILBUD_LES` |
| GET | `/companies/{company_id}/parties` | `RESKONTRO_LES` |
| POST | `/companies/{company_id}/parties` | `RESKONTRO_SKRIV`, `KONTAKT_SKRIV` |
| GET | `/companies/{company_id}/parties/{party_id}` | `RESKONTRO_LES` |
| PUT | `/companies/{company_id}/parties/{party_id}/contact` | `KONTAKT_SKRIV` |
| GET | `/companies/{company_id}/parties/{party_id}/items` | `RESKONTRO_LES` |
| GET | `/companies/{company_id}/parties/{party_id}/matches` | `RESKONTRO_LES` |
| GET | `/companies/{company_id}/products` | `PRODUKT_LES` |
| POST | `/companies/{company_id}/products` | `PRODUKT_SKRIV` |
| PUT | `/companies/{company_id}/products/{number}` | `PRODUKT_SKRIV` |
| GET | `/companies/{company_id}/quotes` | — |
| POST | `/companies/{company_id}/quotes` | — |
| GET | `/companies/{company_id}/quotes/{quote_id}` | `TILBUD_LES` |
| PUT | `/companies/{company_id}/quotes/{quote_id}` | `TILBUD_SKRIV` |
| POST | `/companies/{company_id}/quotes/{quote_id}/order` | `TILBUD_SKRIV` |
| GET | `/companies/{company_id}/quotes/{quote_id}/pdf` | `TILBUD_LES` |
| POST | `/companies/{company_id}/quotes/{quote_id}/status` | `TILBUD_SKRIV` |
| POST | `/companies/{company_id}/reskontro/entries/{entry_id}/terms` | `RESKONTRO_SKRIV` |
| POST | `/companies/{company_id}/reskontro/matches` | `RESKONTRO_SKRIV` |
| DELETE | `/companies/{company_id}/reskontro/matches/{match_id}` | `RESKONTRO_SKRIV` |
| GET | `/companies/{company_id}/reskontro/overview` | `RESKONTRO_LES` |

## Further reading (Norwegian)

- [faktura](/teknisk/faktura/)
- [purring](/teknisk/purring/)
- [produkter](/teknisk/produkter/)
- [reskontro](/teknisk/reskontro/)
- [ehf](/teknisk/ehf/)
