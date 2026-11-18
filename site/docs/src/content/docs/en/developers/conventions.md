---
title: Conventions
description: Amounts, dates, errors, rate limiting, idempotency and pagination. The rules that apply across all endpoints.
sidebar:
  order: 5
---

## Amounts: integer øre

All amount fields ending in `_ore` are **integers in øre**. NOK 1,250.00 is `125000`. There are no floats in the API. **Positive is debit, negative is credit**, in responses too. Amounts in foreign currency are carried in the currency's smallest unit (cents), and a voucher is always authoritative in NOK.

Quantities are given in thousandths: 2.5 units is `"quantity_milli": 2500`. Hours are recorded in whole minutes.

## Dates

Dates are `YYYY-MM-DD`. Timestamps are RFC 3339. A fiscal year is a calendar year, and a voucher in a locked period is rejected.

## Errors

An error always has the shape

```json
{ "error": "text meant for a human" }
```

| Status | When |
| --- | --- |
| `400` | Invalid content or a broken business rule. The message says what is wrong and often what to do |
| `401` | The token is missing or invalid |
| `403` | You have access, but the role lacks the right (the error text is the right's name), or the subscription has expired and the call changes something |
| `404` | Does not exist, **or** no access |
| `429` | Rate limiting for integrations |
| `500` | `{"error": "internal error"}`. Details appear only in the server log |

Messages are in Norwegian where they are meant for the end user. Read `error` as prose. Do not match against the text in code, except for the right name in `403`. An invalid JSON body or an invalid query field is rejected by the framework before the business logic, with `400` or `422`.

## Rate limiting

Integrations have a per-minute limit: default **120**, adjustable per integration. Over the limit the API answers `429` with `{"error": "for mange kall — vent litt"}` ("too many calls, wait a moment"). Signed-in use by humans is not limited.

The limit is a token bucket **per process**, refilled continuously, with a capacity of one minute's budget. With several replicas the effective limit is per replica. That is a deliberate trade-off, not an oversight (see [frugality](/teknisk/frugality/)). Build the client with backoff and retry on `429`.

## Idempotency

There is no general `Idempotency-Key`. Where repeated calls are a real danger, the endpoint has its own mechanism:

- **Uploading vouchers**: send `?sha256=<hex>` with the file's SHA-256. The server rejects (a) content it already has, so a retry after a lost response does not become two vouchers, and (b) a hash that does not match the bytes. The response has `existing: true` when the content was already known.
- **Invoice numbers** are gap-free and issued in the same transaction as the voucher. An invoice is immutable once issued. A correction is a credit note and a new invoice.
- **Bank statements** are identified by content hash, so importing the same file twice is a no-op.
- **SAF-T import** logs the content hash, and the same bytes can never be imported twice.
- **Dispatches** use the dispatch's id as `Nats-Msg-Id`, so the message is never delivered twice.

For everything else: read the state before resending after a lost response.

## Pagination

There is **no general pagination** today, and many list endpoints fetch everything. Some have `limit`:

| Endpoint | Parameter |
| --- | --- |
| `GET …/inbox` | `limit` (1 to 200, newest first), `status` |
| `GET …/parties` | `limit`, `q`, `sort` |
| `GET …/anchors` | `limit` |
| Account specification and posting specification | `limit` (without `limit` the whole period comes back) |

This is an open item and not a design decision. Restrict queries with `from` and `to`.

## Downloads and formats

Some endpoints answer with files: `…/invoices/{id}/pdf` (PDF), `…/invoices/{id}/ehf` (UBL/EHF XML), `…/reports/saft` (SAF-T XML), `…/reports/revisjon?format=tekst` (plain text) and `…/payments/runs/{id}/file` (pain.001). Reports such as the account specification can give CSV. Documents are read with hash verification, so a damaged file gives an error instead of silently wrong content.
