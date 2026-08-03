# API-referanse

Web-et er produktet og API-et er plattformen: **alt portalen gjør, gjør
den over dette API-et**, og et menneske og en integrasjon kaller de
samme endepunktene med de samme reglene (docs/integrations.md).

Denne oversikten er generert fra rutetabellen i
`crates/regnmed-api/src/lib.rs` og oppdateres når den endres. Hvert
endepunkt er dokumentert i sitt eget fagdokument — kolonnen «Covers» i
[README.md](README.md) peker dit.

## Felles regler

- **Autentisering**: `Authorization: Bearer <token>` fra den
  konfigurerte OIDC-issueren. Tokenet beviser identitet; tilgangen
  ligger i regnmeds base (docs/auth.md).
- **Ingen tilgang gir `404`, ikke `403`.** En som ikke har tilgang skal
  ikke lære at selskapet finnes. `403` betyr «du har tilgang, men ikke
  nok» — typisk en revisor (`les`) som prøver å endre noe.
- **Beløp er heltall øre** i alle felter som ender på `_ore`. Positivt
  er debet, negativt er kredit — også i svarene.
- **Datoer er `YYYY-MM-DD`**, tidspunkter er RFC 3339.
- **Feil** har formen `{"error": "..."}` med en melding ment for et
  menneske. Meldingene er på norsk der de er ment for sluttbrukeren.
- **Ratebegrensning** gjelder integrasjoner (`429` over grensen);
  innlogget bruk begrenses ikke.

## Stabilitet

Endepunktene under er i bruk av portalen og regnes som stabile: felter
kan komme til, men eksisterende felter fjernes eller endrer betydning
bare med et varsel i CHANGELOG og en overgangsperiode. Nye endepunkter
dukker opp med hver sak i ROADMAP.md.

## Endepunkter

### Identitet og tilgang

| Endepunkt | Metoder |
| --- | --- |
| `/companies/{company_id}/attestering/policy` | GET POST |
| `/companies/{company_id}/integrations` | GET POST |
| `/companies/{company_id}/integrations/log` | GET |
| `/companies/{company_id}/integrations/{integration_id}/revoke` | POST |
| `/companies/{company_id}/members` | GET |
| `/companies/{company_id}/access` | GET |
| `/companies/{company_id}/access/history` | GET |
| `/companies/{company_id}/access/{person_id}` | PUT DELETE |
| `/companies/{company_id}/access/{person_id}/restore` | POST |
| `/companies/{company_id}/invitations` | GET POST |
| `/companies/{company_id}/invitations/{invitation_id}` | DELETE |
| `/companies/{company_id}/invitations/{invitation_id}/resend` | POST |
| `/companies/{company_id}/roles` | GET POST |
| `/companies/{company_id}/roles/history` | GET |
| `/companies/{company_id}/roles/{role_id}` | PUT |
| `/companies/{company_id}/roles/{role_id}/deactivate` | POST |
| `/companies/{company_id}/roles/{role_id}/restore` | POST |
| `/companies/{company_id}/platform-access` | GET |
| `/firms/{firm_id}/platform-access` | GET |
| `/me` | GET |

### Plattform (docs/auth.md §8)

Krever aktiv plattformrolle (`systemadmin`/`support`); hvert kall
logges og loggen er synlig for selskapet/byrået det gjaldt. Ingen av
endepunktene når noe selskaps hovedbok.

| Endepunkt | Metoder |
| --- | --- |
| `/platform/members` | GET POST |
| `/platform/members/{member_id}` | DELETE |
| `/platform/companies` | GET |
| `/platform/firms` | GET |
| `/platform/users` | GET |
| `/platform/customers` | GET |
| `/platform/users/{person_id}/companies/{company_id}` | POST |
| `/platform/users/{person_id}/firms/{firm_id}` | POST |

### Selskap og onboarding

| Endepunkt | Metoder |
| --- | --- |
| `/companies` | POST |
| `/companies/{company_id}/accounts/{account_number}/reskontro` | PUT |
| `/companies/{company_id}/anchors` | GET |
| `/companies/{company_id}/anchors/verify` | GET |
| `/companies/{company_id}/assets` | GET POST |
| `/companies/{company_id}/assets/depreciate` | POST |
| `/companies/{company_id}/assets/saldo` | GET |
| `/companies/{company_id}/assets/{asset_id}/dispose` | POST |
| `/companies/{company_id}/assets/{asset_id}/runs` | GET |
| `/companies/{company_id}/attachments/{attachment_id}` | GET |
| `/companies/{company_id}/dividends` | GET POST |
| `/companies/{company_id}/bank/matches` | POST |
| `/companies/{company_id}/bank/matches/{bank_transaction_id}` | DELETE |
| `/companies/{company_id}/bank/reconciliation` | GET |
| `/companies/{company_id}/bank/statements` | POST |
| `/companies/{company_id}/budgets` | GET POST |
| `/companies/{company_id}/budgets/{budget_id}` | GET |
| `/companies/{company_id}/budgets/{budget_id}/fastsett` | POST |
| `/companies/{company_id}/budgets/{budget_id}/lines` | PUT |
| `/companies/{company_id}/currency/rates` | GET POST |
| `/companies/{company_id}/currency/rates/fetch` | POST |
| `/companies/{company_id}/currency/regulate` | POST |
| `/companies/{company_id}/dimensions` | GET POST |
| `/companies/{company_id}/dimensions/{kind}/{code}` | PUT |
| `/companies/{company_id}/engagement-requests` | POST |
| `/companies/{company_id}/engagements` | GET |
| `/companies/{company_id}/engagements/{engagement_id}/end` | POST |
| `/companies/{company_id}/expenses` | GET |
| `/companies/{company_id}/expenses/kjoring` | POST |
| `/companies/{company_id}/expenses/utlegg` | POST |
| `/companies/{company_id}/expenses/{expense_id}/approve` | POST |
| `/companies/{company_id}/expenses/{expense_id}/pay` | POST |
| `/companies/{company_id}/expenses/{expense_id}/receipt` | GET |
| `/companies/{company_id}/expenses/{expense_id}/reject` | POST |
| `/companies/{company_id}/import/contacts` | POST |
| `/companies/{company_id}/import/open-items` | POST |
| `/companies/{company_id}/import/saft` | POST |
| `/companies/{company_id}/import/saft/analyze` | POST |
| `/companies/{company_id}/inbox` | GET POST |
| `/companies/{company_id}/inbox/mail` | GET |
| `/companies/{company_id}/inbox/mail/{mail_id}/reject` | POST |
| `/companies/{company_id}/inbox/mail/{mail_id}/release` | POST |
| `/companies/{company_id}/inbox/settings` | GET |
| `/companies/{company_id}/inbox/settings/address` | POST |
| `/companies/{company_id}/inbox/settings/senders` | POST |
| `/companies/{company_id}/inbox/settings/senders/{sender_id}` | DELETE |
| `/companies/{company_id}/inbox/{document_id}/attester` | POST |
| `/companies/{company_id}/inbox/{document_id}/attestering` | GET |
| `/companies/{company_id}/inbox/{document_id}/avvis` | POST |
| `/companies/{company_id}/inbox/{document_id}/bokfor` | POST |
| `/companies/{company_id}/inbox/{document_id}/content` | GET |
| `/companies/{company_id}/inbox/{document_id}/ehf` | GET |
| `/companies/{company_id}/inbox/{document_id}/forslag` | GET |
| `/companies/{company_id}/inventory` | GET |
| `/companies/{company_id}/inventory/count` | POST |
| `/companies/{company_id}/inventory/movements` | GET POST |
| `/companies/{company_id}/invoice-templates` | GET POST |
| `/companies/{company_id}/invoice-templates/{template_id}` | PUT |
| `/companies/{company_id}/invoice-templates/{template_id}/generate` | POST |
| `/companies/{company_id}/invoice-templates/{template_id}/runs` | GET |
| `/companies/{company_id}/invoices` | GET POST |
| `/companies/{company_id}/invoices/overdue` | GET |
| `/companies/{company_id}/invoices/{invoice_id}/credit-note` | POST |
| `/companies/{company_id}/invoices/{invoice_id}/ehf` | GET |
| `/companies/{company_id}/invoices/{invoice_id}/pdf` | GET |
| `/companies/{company_id}/invoices/{invoice_id}/reminders` | GET POST |
| `/companies/{company_id}/invoices/{invoice_id}/reminders/{reminder_id}` | GET |
| `/companies/{company_id}/invoices/{invoice_id}/reminders/{reminder_id}/send` | POST |
| `/companies/{company_id}/invoices/{invoice_id}/send` | POST |
| `/companies/{company_id}/invoices/{invoice_id}/utsendelser` | GET |
| `/companies/{company_id}/mva/terminordning` | GET POST |
| `/companies/{company_id}/ocr/files` | POST |
| `/companies/{company_id}/ocr/payments` | GET |
| `/companies/{company_id}/opening-balance` | POST |
| `/companies/{company_id}/orders` | GET POST |
| `/companies/{company_id}/orders/{order_id}/invoice` | POST |
| `/companies/{company_id}/orders/{order_id}/pdf` | GET |
| `/companies/{company_id}/parties` | GET POST |
| `/companies/{company_id}/parties/{party_id}/contact` | PUT |
| `/companies/{company_id}/parties/{party_id}/items` | GET |
| `/companies/{company_id}/payments/payable` | GET |
| `/companies/{company_id}/payments/runs` | GET POST |
| `/companies/{company_id}/payments/runs/{run_id}/approve` | POST |
| `/companies/{company_id}/payments/runs/{run_id}/cancel` | POST |
| `/companies/{company_id}/payments/runs/{run_id}/file` | GET |
| `/companies/{company_id}/payments/runs/{run_id}/settle` | POST |
| `/companies/{company_id}/period-lock` | GET |
| `/companies/{company_id}/products` | GET POST |
| `/companies/{company_id}/products/{nummer}` | PUT |
| `/companies/{company_id}/quotes` | GET POST |
| `/companies/{company_id}/quotes/{quote_id}` | PUT |
| `/companies/{company_id}/quotes/{quote_id}/order` | POST |
| `/companies/{company_id}/quotes/{quote_id}/pdf` | GET |
| `/companies/{company_id}/quotes/{quote_id}/status` | POST |
| `/companies/{company_id}/reports/aksjonaeroppgave` | GET |
| `/companies/{company_id}/reports/avvik` | GET |
| `/companies/{company_id}/reports/balanse` | GET |
| `/companies/{company_id}/reports/bokforingsspesifikasjon` | GET |
| `/companies/{company_id}/reports/kontospesifikasjon` | GET |
| `/companies/{company_id}/reports/mva` | GET |
| `/companies/{company_id}/reports/mva-melding` | GET |
| `/companies/{company_id}/reports/nokkeltall` | GET |
| `/companies/{company_id}/reports/prosjekt` | GET |
| `/companies/{company_id}/reports/resultat` | GET |
| `/companies/{company_id}/reports/revisjon` | GET |
| `/companies/{company_id}/reports/saft` | GET |
| `/companies/{company_id}/reports/saldobalanse` | GET |
| `/companies/{company_id}/reskontro/matches` | POST |
| `/companies/{company_id}/reskontro/matches/{match_id}` | DELETE |
| `/companies/{company_id}/settings` | GET |
| `/companies/{company_id}/share-events` | GET POST |
| `/companies/{company_id}/shareholders` | GET POST |
| `/companies/{company_id}/shareholders/transaction-types` | GET |
| `/companies/{company_id}/shareholders/{shareholder_id}/contact` | PUT |
| `/companies/{company_id}/timesheet` | GET POST |
| `/companies/{company_id}/timesheet/invoice` | POST |
| `/companies/{company_id}/timesheet/lock` | GET |
| `/companies/{company_id}/timesheet/summary` | GET |
| `/companies/{company_id}/timesheet/unbilled` | GET |
| `/companies/{company_id}/timesheet/{entry_id}` | PUT |
| `/companies/{company_id}/vouchers` | GET |
| `/companies/{company_id}/vouchers/{voucher_id}/attachments` | GET POST |
| `/directory/firms` | GET |
| `/firms` | POST |
| `/firms/mine` | GET |
| `/firms/{firm_id}/access` | GET |
| `/firms/{firm_id}/access/history` | GET |
| `/firms/{firm_id}/access/{person_id}` | PUT DELETE |
| `/firms/{firm_id}/access/{person_id}/restore` | POST |
| `/firms/{firm_id}/clients` | GET |
| `/firms/{firm_id}/invitations` | GET POST |
| `/firms/{firm_id}/invitations/{invitasjon_id}` | DELETE |
| `/firms/{firm_id}/invitations/{invitasjon_id}/resend` | POST |
| `/firms/{firm_id}/requests` | GET |
| `/firms/{firm_id}/requests/{request_id}/decision` | POST |
| `/registry/enheter/{orgnr}` | GET |

### Tillit og verifikasjon

| Endepunkt | Metoder |
| --- | --- |
| `/health` | GET |

### Portalen selv

`/`, `/callback`, `/assets/*`, `/ny`, `/portal-config` og `/auth/token`
betjener nettportalen (docs/portal.md), sammen med PWA-filene
`/manifest.webmanifest`, `/sw.js` og `/icon-192.png` / `/icon-512.png`.
De er ikke en del av integrasjons-API-et.
