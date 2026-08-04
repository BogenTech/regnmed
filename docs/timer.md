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
  entries; `TIMER_SKRIV_ALLE` corrects anyone's) until either:
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

## Web API (the rettigheter decide — docs/auth.md)

| Endpoint | Purpose | Rettighet |
| --- | --- | --- |
| `GET /companies/{id}/timesheet?from=&to=` | entries in range + lock status | `TIMER_LES_EGNE` — own rows only unless `TIMER_LES_ALLE` |
| `POST /companies/{id}/timesheet` | record (dato, minutter, beskrivelse, prosjekt?, fakturerbar?, timesats_ore?) | `TIMER_SKRIV_EGNE` |
| `PUT/DELETE …/timesheet/{eid}` | correct/remove an entry | `TIMER_SKRIV_EGNE` (own) / `TIMER_SKRIV_ALLE` (anyone's) |
| `GET …/timesheet/summary?from=&to=` | per-prosjekt totals + unbilled value | `TIMER_RAPPORT_LES` |
| `GET …/timesheet/unbilled` | fakturagrunnlaget, grouped, w/ per-person breakdown | `TIMER_RAPPORT_LES` |
| `POST …/timesheet/invoice` | bill (party_no; optional prosjekt/through/entry_ids/vat_code/dates) | `TIMER_FAKTURER` |
| `GET/PUT …/timesheet/lock` | månedslås (insert-only history) | `TIMER_LES_EGNE` / `TIMER_LAAS` |

An ansatt therefore sees **only their own hours**; `TIMER_LES_ALLE`
(bokforing, revisor, admin — hours are billing evidence, the audit
reads everyone's like lønn) unlocks the whole team. The `_ALLE` →
`_EGNE` implication holds for company-defined roles too — it is a
property of the rettighet, not of the role kind.

Unbilled groups carry the project's **kunde** when the dimension is
linked (#80, docs/dimensjoner.md) — a *suggested* recipient, never
automation: billing still takes an explicit `party_no` from the caller.
Each group also names **whose** hours it holds (per-person minutter +
entry ids), and `entry_ids` on the invoice call bills exactly that
selection: every chosen id must still be billable and unbilled (a stale
selection fails whole, never silently bills less), and the entries are
marked fakturert in the invoice transaction itself — **selection and
lock are one step**, there is never a chosen-but-editable window. For
approval AHEAD of billing, the månedslås is the tool: lock the month,
then bill (billing locked hours is the one allowed change).

Portal: the Timer section opens with **ukegridet** — rows are
prosjekter, columns are the days, a cell is that day's hours. Cells
save themselves on blur through the ordinary entry endpoints (flexible
input: 7,5 / 7.5 / 7:30), Enter and arrow keys move like a
spreadsheet, rows pre-populate from last week's projects and «Kopier
forrige uke» fills an empty week; locked days render disabled, billed
cells show their faktura. Beskrivelse/sats per line live in the detail
panel under the grid (focused cell); on small screens the grid
collapses to a one-day view with day chips. Below: per-prosjekt totals
and the ufakturerte card with per-person lines, checkboxes and
«Fakturer valgte» (plus the #80 suggestion buttons and the everything
path). Project-shaped cards link to the dimension registry, and with
an empty registry the project picker says so rather than disappearing
(docs/dimensjoner.md).

## Deliberately not (yet)

- Bemanningsplanlegging and attestering-flyt — the lock covers the
  integrity need first (the issue's own scoping).
- Default timesats per prosjekt/kunde — sats is entered per entry in v1
  (the grid softens this: new cells inherit fakturerbar/sats from the
  row's newest line).
- **Per-project access** (a "prosjektleder" who may only see/bill ONE
  project's hours): rights are company-scoped; a custom role holding
  `TIMER_LES_ALLE` + `TIMER_FAKTURER` covers the person, not the
  project boundary. If a real need arrives it is a new scoping on the
  rettighet model, not a bundle tweak.

## Where it is tested

- `regnmed-api/tests/grupper/timesheet.rs` (real Postgres, also CI) — record/
  edit/delete over HTTP, validation (sats required when fakturerbar,
  unknown/avsluttet prosjekt), week view and summary sums, the month
  lock rejecting changes at BOTH the API and the trigger layer, billing
  locked hours into an invoice whose revenue entry carries the prosjekt
  dimension (chain verifies), fakturerte timer immutable and never
  rebilled, invoice link visible in the week view. The rights test
  (`timer_rights_mean_what_they_say`, seen failing before the guards
  landed) pins visibility (egne vs `TIMER_LES_ALLE`), correction via a
  custom role holding `TIMER_SKRIV_ALLE`, and `TIMER_FAKTURER` on
  billing. Selective billing
  (`a_selection_of_the_grunnlag_bills_and_locks_exactly_itself`) pins
  the per-person grunnlag, billing one person's hours while the rest
  stay unbilled, the DB lock on the selection, and that a stale
  selection fails whole.
