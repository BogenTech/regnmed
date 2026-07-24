# Timeføring

Hours are the inventory of tjenesteytende SMB-er. The minimal honest
core (#38): integer minutes, a month lock that turns hours into
evidence, and a fakturagrunnlag that bills through the ordinary invoice
path.

## Guarantees

- **Minutes are integers** (1..=1440 per entry) — no floats, same
  discipline as øre. Displayed hours are presentation.
- **Editable until locked or billed, then immutable — enforced in the
  database.** Entries are working data (edit/delete freely, own
  entries; admins may correct anyone's) until either:
  - the **month lock** passes them: `timesheet_lock` is an insert-only
    history exactly like period_lock (newest row wins, reopening is an
    audited insert), and a trigger rejects insert/update/delete of
    entries dated on or before the lock — independently of the API; or
  - they are **fakturert**: the invoice link is one-way, and the same
    trigger rejects any later change. The only change allowed on a
    locked entry is the pure billing marker (lock hours for lønn, then
    bill them).
- **Fakturagrunnlaget bills through the ordinary path**: unbilled
  billable hours group per (prosjekt, timesats) into invoice lines —
  quantity in milli-hours (90 min → 1,5 t), the **prosjekt dimension
  carried onto the revenue line** (docs/dimensjoner.md) — issued via
  the normal atomic invoice transaction (gap-free number, KID, posting,
  stored PDF), with every entry marked fakturert in that same
  transaction. Nothing is ever billed twice.
- Prosjekt references must be active dimensions (avsluttet rejects, as
  everywhere).

## Web API (writes require bokforing; lock requires admin)

| Endpoint | Purpose |
| --- | --- |
| `GET /companies/{id}/timesheet?from=&to=` | entries in range + lock status |
| `POST /companies/{id}/timesheet` | record (dato, minutter, beskrivelse, prosjekt?, fakturerbar?, timesats_ore?) |
| `PUT/DELETE …/timesheet/{eid}` | own entries (admins: anyone's) |
| `GET …/timesheet/summary?from=&to=` | per-prosjekt totals + unbilled value |
| `GET …/timesheet/unbilled` | fakturagrunnlaget, grouped |
| `POST …/timesheet/invoice` | bill (party_no; optional prosjekt/through/vat_code/dates) |
| `GET/PUT …/timesheet/lock` | månedslås (insert-only history) |

Portal: the Timer section — min uke with week navigation and quick
registration, per-prosjekt totals, ufakturerte timer with "Lag
faktura".

## Deliberately not (yet)

- Bemanningsplanlegging and attestering-flyt — the lock covers the
  integrity need first (the issue's own scoping).
- Default timesats per prosjekt/kunde — sats is entered per entry in v1.

## Where it is tested

- `regnmed-api/tests/timesheet.rs` (real Postgres, also CI) — record/
  edit/delete over HTTP, validation (sats required when fakturerbar,
  unknown/avsluttet prosjekt), week view and summary sums, the month
  lock rejecting changes at BOTH the API and the trigger layer, billing
  locked hours into an invoice whose revenue entry carries the prosjekt
  dimension (chain verifies), fakturerte timer immutable and never
  rebilled, invoice link visible in the week view.
