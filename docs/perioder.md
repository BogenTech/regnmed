# Periodelåsing og bilagsvedlegg

The last two pieces of the lovpålagte kjerne: ajourhold (locked periods)
and oppbevaringsplikt for dokumentasjon (bokføringsloven §13).

## Periodelåsing

A company's lock is a single "locked through" date; postings dated on or
before it are rejected, and corrections go into an open period as
reversing vouchers (which the ledger already mandates).

- **Insert-only history**: every lock change is a new row — advancing
  after a completed termin, and any reopening, stays in the audit trail
  forever. The current lock is simply the latest row.
- **Two enforcement layers**, like everything else: the posting path
  rejects with a clear message, and a database trigger
  (`forbid_locked_period_posting`) independently blocks even
  hand-inserted vouchers at the SQL level.
- **Reopening requires admin**: an accountant (bokforing) can advance the
  lock; moving it backwards is an admin-only act — and it is recorded,
  not hidden.

- **A period that has not ended cannot be locked**: a date after today
  is refused, because it would reject today's own postings.
- **One change at a time per company**: the read of the current lock and
  the insert of the next run in one transaction under a per-company
  advisory lock (`lock_period_changes`), which the year-end closing takes
  too — two changes at the same moment cannot both pass their checks.
- **Reopenings are named**: the history marks each row that moved the
  lock back (`gjenapnet`, computed from the row before it — nothing new
  is stored), and the portal shows it.
- Refusals are in Norwegian, with dates as dd.mm.yyyy: they are shown to
  the person who tried.

API: `GET`/`PUT /companies/{id}/period-lock`.

Portal: Administrasjon → Periode. The next step is one button — lock
through the end of last month — with any other date in a labelled field
limited to today. Both are confirmed first; a date before the current
lock is called a reopening and is offered to admins only. The form is
hidden without `PERIODE_LAAS`.

## Bilagsvedlegg

Attachments (PDF/images) bind dokumentasjon to vouchers:

- **Append-only** like the ledger — the same database trigger family
  rejects UPDATE/DELETE/TRUNCATE, and the app role holds only
  INSERT/SELECT. Attaching to a voucher in a *locked* period is allowed:
  completing documentation is legitimate; altering it is not.
- **Content SHA-256** is computed at upload, stored, returned to the
  uploader, and re-verified on every download and by
  `regnmed verify-ledger` (which now also re-hashes all attachments) — a
  swapped or altered document is detected even if the triggers were
  bypassed at the DBA level.
- **Deliberate limitation** (documented, not hidden): the attachment
  *set* of a voucher is not yet inside the voucher hash chain, because
  documentation legitimately arrives after posting. Anchoring now
  exists ([anchoring.md](anchoring.md)); binding attachment *sets* into
  the anchor is the planned leaf-format v2 follow-up described there.
- Storage is in Postgres (bytea) — right for SMB volumes and the
  frugality budget; object storage is a swap-in later if sizes demand it.

API: `POST/GET /companies/{id}/vouchers/{vid}/attachments`,
`GET /companies/{id}/attachments/{aid}` (download, hash-checked), plus
`GET /companies/{id}/vouchers` so the web can find voucher ids.

## Where it is tested

`regnmed-api/tests/grupper/period_attachments.rs` (real Postgres, also CI): both
lock layers reject a back-dated posting (app error and raw-SQL trigger),
accountant-vs-admin reopening rules with the audit trail (the
reopening named in the history, the refusals in Norwegian, a date after
today refused), upload/download
round-trip with hash equality, UPDATE rejected, DBA-level content
tampering caught by `verify_attachments` — and clean verification after
restore.
