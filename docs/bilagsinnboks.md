# Bilagsinnboks

The daily loop between a business and its regnskapsfører: the client
uploads dokumentasjon (receipts, supplier invoices, contracts) the
moment it exists; the accountant turns each document into a posted
voucher — or rejects it with a note that goes back to the client.

## Honesty rules (enforced in the database, migration 0015)

- **Content is immutable from arrival.** SHA-256 stored at upload;
  column-level grants plus a trigger reject any change to the content
  columns. Downloads re-check the hash on the way out.
- **A decision is one-way.** `ny` → `bokfort` (with the voucher id) or
  `ny` → `avvist` (note required). Re-deciding is rejected by trigger —
  even an application bug cannot flip a decided document.
- **Nothing is deleted.** A rejected document and its note remain part
  of the story, like everything else in regnmed.

## Bokføring is atomic

`POST /companies/{id}/inbox/{doc}/bokfor` takes a voucher draft and, in
**one transaction**: posts the voucher through the normal path (hash
chain, gap-free numbers, period-lock check), copies the document into
the append-only `attachment` table bound to the new voucher — this is
the moment oppbevaringsplikt begins ([perioder.md](perioder.md)) — and
marks the inbox entry `bokfort`. A failed posting (unbalanced draft,
locked period, unknown account) rolls back everything: the document
stays `ny`, no voucher, no attachment. The integration test proves
both directions, and that the attachment carries the exact uploaded
bytes (same SHA-256).

## Endpoints

| Endpoint | Access |
| --- | --- |
| `POST /companies/{id}/inbox?filename=` (bytes) | admin/bokforing |
| `GET /companies/{id}/inbox[?status=ny]` | any (revisor reads) |
| `GET /companies/{id}/inbox/{doc}/content` | any, hash-checked |
| `POST /companies/{id}/inbox/{doc}/bokfor` (voucher draft) | admin/bokforing |
| `POST /companies/{id}/inbox/{doc}/avvis` (`{note}`) | admin/bokforing |

Portal: the Bilag section opens with the inbox — upload, pending list
with Bokfør (inline voucher form) and Avvis, and the recent decisions
under it.

## Deliberately not yet

OCR/tolkning of uploaded documents (suggesting account and amount) is a
later enhancement; it must only ever *suggest* — the accountant decides,
and the posted voucher is what carries legal meaning. E-mail-in and
mobile capture ride on the same endpoint when they come.

## Where it is tested

- `crates/regnmed-api/tests/innboks.rs` (real Postgres, also in CI):
  upload → list → bokfør posts voucher + attachment + status in one
  transaction (attachment bytes hash-identical to the upload); an
  unbalanced draft fails and leaves the document undecided; re-deciding
  refused; rejection requires a note; revisor can read but not decide;
  outsiders 404; the chain verifies over the new voucher.
