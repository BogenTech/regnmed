# Migration: SAF-T import

"Switching is painless" is the growth lever: every Norwegian accounting
system must export SAF-T Financial, so one importer covers moving in
from Visma, Tripletex, Fiken, Conta, PowerOffice, Unimicro and the rest.
`POST /companies/{id}/import/saft` (admin only) — the portal offers it on
an empty company's dashboard ("Kom fra et annet system?").

## The rules that keep migration honest

- **Empty ledger only, one transaction**: import is allowed only while
  the chain head sits at genesis, and the entire file lands in a single
  database transaction — any error rolls back everything, and a re-run
  cannot duplicate. Migration happens before day-to-day bookkeeping.
- **History becomes real vouchers**: every SAF-T transaction is posted
  through the normal posting path — our gap-free numbers, hash chain v2
  from genesis — into a dedicated `IMP` journal, with the source
  system's transaction id preserved in the description. `verify-ledger`
  covers imported history exactly like native history.
- **Opening balances must balance**: the file's account opening balances
  become one `Åpningsbalanse` voucher dated the day before history
  starts, and they must sum to zero — a partial export is refused with
  the discrepancy, never papered over.
- **Reskontro conservatively**: an account is flagged kunde/leverandør
  only when *every* line on it carries the matching party; mixed
  accounts (or accounts with party-less opening balances) are imported
  without links and each case is a warning in the report. Party ids are
  kept when numeric; others are renumbered from 90000 with the mapping
  reported. Full reskontro migration polish is the mapping wizard (#18).
- **Unknown VAT codes are dropped with a warning** (regnmed's codes are
  the SAF-T standard codes, so conforming files map 1:1); non-4-digit
  account ids are refused pending the kontoplan wizard (#18).

## Where it is tested

- `regnmed-core/src/saft_import.rs` — the **round-trip test**: a file
  rendered by our own exporter parses back with identical accounts,
  parties, signs and balances. The parser is tolerant (path-based, extra
  elements skipped) in the camt.053 parser's style.
- `regnmed-api/tests/saft_migration.rs` (real Postgres, also CI): a
  foreign file imports over HTTP into an empty company; the chain
  verifies from genesis; the trial balance equals the foreign system's
  closing balances konto for konto; customer numbers survive; deferred
  reskontro flags are warned; non-admins and re-imports are refused.
