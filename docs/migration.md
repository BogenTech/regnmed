# Migration: SAF-T import

"Switching is painless" is the growth lever: every Norwegian accounting
system must export SAF-T Financial, so one importer covers moving in
from Visma, Tripletex, Fiken, Conta, PowerOffice, Unimicro and the rest.
`POST /companies/{id}/import/saft` (admin only) — the portal offers it on
an empty company's dashboard ("Kom fra et annet system?").

## The rules that keep migration honest

- **Before day-to-day bookkeeping, one transaction per file**: import is
  allowed into an empty ledger (chain head at genesis) — and after that
  for as long as the ledger contains nothing but `IMP`-journal vouchers,
  so a history that arrives as several files (below) can be imported one
  file at a time. The first ordinary voucher closes the door for good.
  Each file lands in a single database transaction — any error rolls
  back that whole file, and a re-run cannot duplicate (a repeated file
  fails the opening-balance reconciliation).
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
  account ids are refused unless the import carries a kontoplan mapping
  (below).

## Flerårig historikk: én fil per regnskapsår

Several source systems export one SAF-T Financial file per fiscal year —
verified 2026-08-06 against a real Conta export (2025 full year, 2026
January–April). The years are therefore imported **one file at a time,
oldest first**, and a follow-up file must continue exactly where the
imported history stopped. Its opening balances are reconciled konto for
konto before anything is written:

- **Balance accounts (class 1–2)** must open at the booked all-time
  balance — they carry over the year end unchanged.
- **Resultat accounts (class 3–9)** are reset at year end by the
  exporting system, while regnmed never posts year-end closings
  (udisponert resultat is derived in the balanse report). Their file
  opening is therefore compared against the booked entries of the
  file's **own fiscal year** (the `regnskapsar` seam): zero at a year
  boundary, the year-to-date sum when one year arrives in several
  files.

Any difference refuses the import with each account and the exact øre
difference named — the numbers are wrong or the files are out of order,
and neither is papered over. No second `Åpningsbalanse` is posted: the
balances are already in the ledger, and the report says
`opening_reconciled` instead of `opening_posted`.

Reconciliation has one blind spot: a file whose period nets to zero on
every account would reconcile cleanly a second time and double-post its
transactions. The insert-only **`saft_import_log`** (migration 0054)
closes the byte-identical case — every imported file is recorded with
its content SHA-256 (of the source XML, before any kontoplan mapping),
and the same content is refused with a plain sentence. The unique
constraint on the log is the second layer: even without the explicit
check, inserting the duplicate row would roll the whole import back.
The log doubles as the audit trail of which files a migrated ledger was
built from: `GET /companies/{id}/import/saft/log` serves it to anyone
who can read the ledger (where the history came from is part of the
history), and the portal's import card lists the files. A *re-export*
of the same period (different bytes, same zero-net content) is not
caught — that is documented honesty, not a promise.

Two findings from the real Conta files that shaped these rules:

- The follow-up file's openings do **not** sum to zero — resultat
  accounts open at zero without any counterpart on equity (the
  resultatdisponering is simply absent). The first-import zero-sum rule
  therefore only applies to the first file.
- Per-year reports still match the source system because regnmed's
  resultat reports select by date range; only the all-time saldo on
  resultat accounts differs from a system that closes its books, and
  the balanse report accounts for exactly that as udisponert resultat.

The portal keeps the import card on the dashboard for as long as the
ledger contains only imported history, and says so.

## Kontoplan wizard (non-NS 4102 charts)

Files from systems with other numbering (5-digit charts, custom ranges,
alphanumeric ids) go through a two-step wizard:

1. `POST /companies/{id}/import/saft/analyze` parses the file (nothing
   is written) and returns every account with a **suggested** NS 4102
   mapping: 4-digit numbers map to themselves; longer digit strings are
   truncated when the first four digits form a plausible account
   (1000–8999); shorter ones are zero-padded; otherwise the account
   *name* is matched against the standard names in the vendored
   næringsspesifikasjon list. Suggestions are heuristics — the
   administrator reviews, corrects and completes them in the portal;
   the human decision is what gets imported.
2. `POST /companies/{id}/import/saft` with a JSON envelope
   `{"file": "<xml>", "mapping": {"15000": "1500", …}}` applies the
   mapping (`regnmed-core::kontoplan::apply_mapping`): line and account
   ids are rewritten, and foreign accounts mapped onto the same NS 4102
   number are **merged** with openings summed. The import's own 4-digit
   validation still guards the result — a half-finished mapping fails
   loudly instead of importing garbage.

## Manual åpningsbalanse (no SAF-T at all)

`POST /companies/{id}/opening-balance` (`{date, lines: [{account,
amount_ore}]}`, admin only) posts one `Åpningsbalanse` voucher through
the normal path, for companies whose old system cannot export SAF-T.
Same honesty rules as the import: empty ledger only, the lines must sum
to zero (refused with the discrepancy named), and reskontro flags on
touched accounts are deferred with a warning — an opening total has no
party breakdown. The portal offers it next to the SAF-T card on empty
companies.

## Kontakter og åpne poster (filtier, #19)

SAF-T flytter hovedboken, men bærer ikke alt et byrå trenger for å
slutte å bruke det gamle systemet: **kontaktopplysninger** (adresse,
e-post, kontonummer) og **åpne reskontroposter** står igjen. Alle de
norske systemene kan eksportere begge deler som CSV i dag, uten
API-nøkler — så filtieren kommer først, akkurat som for bank
(docs/bank.md). API-tieren (Tripletex, Fiken, Visma eAccounting,
PowerOffice, Conta) krever avtale og nøkler per leverandør og er
dokumentert som neste steg, ikke lovet her.

Layouten leses av **kolonneoverskriftene**, ikke av en profil per
leverandør (`regnmed-core::migreringcsv`, samme grep som bank-CSV):
overskriftsvokabularet endrer seg langsommere enn produktnavnene, og
en fil vi ikke forstår feiler høyt med kolonnene vi faktisk så.

| Endepunkt | Hva |
| --- | --- |
| `POST …/import/contacts?kind=kunde\|leverandor` | Kontaktliste. Idempotent: match på orgnr → nummer → navn. Et numerisk kundenr blir partsnummeret (10001 forblir 10001). |
| `POST …/import/open-items?kind=&konto=&motkonto=&dato=&preview=` | Åpne poster. `preview=true` leser filen, slår opp partene og sjekker saldoen uten å bokføre noe. |

To detaljer som er lette å ta feil av, og som derfor er testet:

- **Restbeløp vinner over fakturabeløp.** Eksporter har ofte begge;
  å importere bruttobeløpet ville blåst opp reskontroen i stillhet.
  Kolonnelisten er en prioritetsrekkefølge, ikke et sett.
- **Retningen bestemmes av parts-typen, ikke av filen.** Kunde =
  debet, leverandør = kredit. Eksportene er uenige med hverandre om
  fortegn, men aldri om hvem som skylder hvem. En kreditnota beholder
  sitt eget negative fortegn gjennom regelen.

### Åpne poster ERSTATTER samlelinjen

Postene blir ETT bilag med én partslinje per post, balansert mot en
motkonto (2050 som standard) — i én transaksjon. Derfor krever
importen at reskontrokontoen står i **null** først, og sier fra med
den faktiske saldoen hvis ikke. Rekkefølgen ved migrering er:

1. Kontakter — så postene har noen å peke på.
2. Åpningsbalanse **uten** 1500/2400 (docs/migration.md over).
3. Åpne poster.

Etterpå er reskontrosaldoen lik summen av de åpne postene fordi det
er de samme radene — ikke fordi noe ble avstemt i etterkant. Importen
setter også reskontro-flagget på kontoen tilbake; åpningsbalansen
utsetter det med vilje, og dette er stedet det kommer igjen.

Portal: Reskontro → «Importer fra et annet system» (forhåndsvisning,
så bokfør).

## Where it is tested

- `regnmed-core/src/saft_import.rs` — the **round-trip test**: a file
  rendered by our own exporter parses back with identical accounts,
  parties, signs and balances. The parser is tolerant (path-based, extra
  elements skipped) in the camt.053 parser's style.
- `regnmed-api/tests/grupper/saft_migration.rs` (real Postgres, also CI): a
  foreign file imports over HTTP into an empty company; the chain
  verifies from genesis; the trial balance equals the foreign system's
  closing balances konto for konto; customer numbers survive; deferred
  reskontro flags are warned; non-admins and re-imports are refused.
  Multi-year: one file per year imports sequentially (Conta-shaped
  openings that do not sum to zero), a mid-year continuation file
  reconciles resultat accounts against the year-to-date sum, a
  mismatched opening is refused with the account and øre difference
  named, a zero-net file re-imported byte-identically is refused by the
  import log (the guard has been seen to fail: with the check disabled
  the unique constraint answers instead), and one ordinary voucher
  closes the import for good.
- `regnmed-core/src/kontoplan.rs` — suggestion heuristics (identity,
  truncation, padding, name match, no-suggestion) and mapping
  application (rewrite, merge with summed openings, 4-digit target
  validation).
- `regnmed-core/src/migreringcsv.rs` — per-layout parser tests
  (Tripletex-style semicolon export with quoted delimiters, English
  export where a type column beats the caller's default, restbeløp
  winning over beløp, credit notes keeping their sign, settled items
  skipped, forfallsdato not stealing the fakturadato column, unknown
  layouts naming the headers).
- `regnmed-api/tests/grupper/migrering.rs` (real Postgres, also CI): contacts
  import twice (created, then updated — idempotent), kundenr surviving
  as party_no, MOD11-validated kontonummer, open-items preview with
  the unknown party listed and nothing posted, the import making the
  1500 balance equal the sum of the items, invoice numbers on the
  entries, a second run refused with the balance in the message,
  supplier items landing on the credit side, and an unreadable file
  naming its columns.
- `regnmed-api/tests/grupper/kontoplan.rs` (real Postgres, also CI): a 5-digit
  chart is refused raw, analyzed with correct suggestions, imported
  with a reviewed mapping including a two-onto-one merge — balances
  land merged and the chain verifies; the manual åpningsbalanse
  refuses unbalanced lines, posts once, and refuses a second time.
