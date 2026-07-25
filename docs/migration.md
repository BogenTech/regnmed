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
  account ids are refused unless the import carries a kontoplan mapping
  (below).

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
- `regnmed-api/tests/saft_migration.rs` (real Postgres, also CI): a
  foreign file imports over HTTP into an empty company; the chain
  verifies from genesis; the trial balance equals the foreign system's
  closing balances konto for konto; customer numbers survive; deferred
  reskontro flags are warned; non-admins and re-imports are refused.
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
- `regnmed-api/tests/migrering.rs` (real Postgres, also CI): contacts
  import twice (created, then updated — idempotent), kundenr surviving
  as party_no, MOD11-validated kontonummer, open-items preview with
  the unknown party listed and nothing posted, the import making the
  1500 balance equal the sum of the items, invoice numbers on the
  entries, a second run refused with the balance in the message,
  supplier items landing on the credit side, and an unreadable file
  naming its columns.
- `regnmed-api/tests/kontoplan.rs` (real Postgres, also CI): a 5-digit
  chart is refused raw, analyzed with correct suggestions, imported
  with a reviewed mapping including a two-onto-one merge — balances
  land merged and the chain verifies; the manual åpningsbalanse
  refuses unbalanced lines, posts once, and refuses a second time.
