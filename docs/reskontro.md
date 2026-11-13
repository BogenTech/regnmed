# Reskontro: kunde- og leverandørspesifikasjon

Bokføringsforskriften §3-1 requires subledgers per customer and supplier.
In regnmed a **party** (kunde/leverandør) is master data with a numeric
party number; ledger entries on reskontro-flagged accounts carry the
party, and the spesifikasjon and åpne poster are pure queries over those
entries.

## The party binding is tamper-evident (hash format v2)

The party number is **inside the voucher hash**. Reassigning a
receivable from one customer to another — the classic subledger fraud —
breaks the chain like any other edit. This required the first hash
format version bump, done as designed (docs/ledger.md):

- Every voucher stores `hash_version`. Existing history is v1 and
  verifies unchanged forever; new postings are v2 (a `"v2"` marker field
  plus the party number per entry, empty when none).
- Mixed-version chains verify from genesis; both formats are frozen and
  pinned by golden tests.

## Rules (enforced at posting, in one place)

- An account flagged `reskontro_kind` ('kunde'/'leverandor') **requires**
  a party of that kind on every entry.
- Ordinary accounts **reject** parties.
- Party numbers are immutable business identifiers (they are hashed);
  names/orgnr stay editable master data. Auto-numbering: kunder from
  10000, leverandører from 50000.

## Åpne poster

`reskontro_match` links an invoice-side entry to a settlement-side entry
for an amount — partial settlements are rows, an item's *remaining* is
always computed (`amount − matched`), never stored. Over-matching a
remainder is rejected; matches are per party and account, cross-company
pairings impossible.

Matches on one party take turns (`pg_advisory_xact_lock` on the party
inside `match_items`): the remainders are read and the match written
under the same lock, so two simultaneous matches cannot both consume
the same open amount. The remainder formula lives in the view
`subledger_entry_open` (migration 0083); new queries read it instead of
writing the formula out again, and the older ones move to it as they
are touched.

A match is undone with `DELETE …/reskontro/matches/{id}` — the vouchers
stand untouched, only the pairing goes. The one exception is a
**currency settlement** (`match_valuta`): its agio voucher was posted in
the same transaction as the match rows, so removing a row would leave
the agio standing against nothing. That refusal is in
`unmatch_items`, and such matches are corrected with a reversal of the
agio voucher.

## Web API (engagement-guarded; mutations require bokforing)

| Endpoint | Purpose |
| --- | --- |
| `GET /companies/{id}/parties?kind=kunde` | spesifikasjon: parter med saldo. Med `limit` (≤ 200) også `overdue_ore` — det som står igjen på partens forfalte fakturaer, etter fakturalistens egen status; uten `limit` er feltet `null`, fordi utregningen går over alle fakturaene og listene som bare trenger navn ikke skal betale for den. `q` treffer eksakt partsnummer eller orgnr, eller del av navn eller e-post; `sort` = `number` (standard, numerisk), `name`, `balance` eller `overdue`, og `desc=true` snur rekkefølgen. Svaret har `total`. Uten `limit`: alle parter, som før |
| `GET /companies/{id}/parties/{pid}` | én part med saldo og forfalt beløp (for en leverandør: poster med passert forfallsdato, docs/betaling.md) |
| `POST /companies/{id}/reskontro/entries/{entry}/terms` | forfall og KID på en leverandørpost (docs/betaling.md) |
| `POST /companies/{id}/parties` | opprett part (partsnummer tildeles). Valgfritt `address`, `email` og `bank_account` i samme kall (krever `KONTAKT_SKRIV`, som `PUT …/contact`). De kontrolleres FØR parten opprettes, så et ugyldig kontonummer etterlater aldri en halvferdig part |
| `GET /companies/{id}/parties/{pid}/items?open=true` | items with matched/remaining |
| `POST /companies/{id}/reskontro/matches` + `DELETE …/{match_id}` | åpne poster matching (valutaoppgjør kan ikke løses opp) |
| `GET /companies/{id}/parties/{pid}/matches` | partens avstemminger, nyeste først, med begge bilag, hvem og når, og `currency_settlement` |
| `GET /companies/{id}/reskontro/overview` | per reskontrokonto hovedbok mot sum parter (samme tall som revisjonsrapportens kontroll 4, `subledger_accounts`), og partene med åpne poster på BEGGE sider (`to_match`, inntil 100, alfabetisk, med `to_match_total`) |
| `PUT /companies/{id}/accounts/{nr}/reskontro` | flag/clear a reskontro account |

The statutory kunde-/leverandørspesifikasjon (per party, per period,
with running saldo and bilagshenvisning) lives with the other
bokføringsforskriften §3-1 reports:
`GET …/reports/kundespesifikasjon` and
`GET …/reports/leverandorspesifikasjon` ([rapporter.md](rapporter.md)).
`…/parties/{pid}/items` stays the åpne poster working view.

In the portal every party has ONE page, under **Kunder** or
**Leverandører** (register w/ search and sorting, party page w/ contact
info). Its «Reskontro: poster og avstemming» panel
(`sections/parties/SubledgerItems.svelte`) shows the open items, the
**suggested matches** (`parties/matching.js`: an exact amount first,
then what is left of a payment against the oldest open invoices — the
server checks every match, this only proposes), a manual pick of one
item on each side with the amount and what stays open, and the history
with «Løs opp». The two sides are named from the party kind: the
matcher always takes the positive remainder as `entry_a`, but that is
the customer's *invoice* and the supplier's *payment* — a fixed
"Faktura" label would point the user the wrong way on the supplier
side.

**Reskontro** is the workbench across both kinds: *Oversikt* (does each
reskontro account agree with the hovedbok, and which parties have
something to match — each links to the party page with the panel
open, `?avstem`), *Parter* (the same register for both kinds), *Import*
(CSV from another system, docs/migration.md) and *Reskontrokontoer*
(flag or clear an account, with what that means). Old links to
`#/c/{id}/reskontro/{party}` land on the party's own page.

## SAF-T

The export now includes `Customers`/`Suppliers` master data with
subledger opening/closing balances, and invoice/payment lines carry
`CustomerID`/`SupplierID` — validated against the official XSD like the
rest of the file.

## Avstemmingen mot hovedboken

At spesifikasjonen og hovedboken er de samme posteringene, følger av
posteringsveien (partskravet er håndhevet i `post_voucher`) — men
reskontro-flagget kan flytte seg etterpå (åpningsbalanse og SAF-T-import
nullstiller det). Revisjonsrapportens **kontroll 4** avstemmer derfor
Σ reskontro mot kontosaldoen konto for konto, og navngir differansen i
øre: [revisjon.md](revisjon.md).

## What this unlocks next

- **Faktura (#13)**: invoices post against parties with KID; OCR
  payments (docs/bank.md) then auto-apply by KID.
- Purring/aldersfordelt saldoliste are queries over åpne poster.

## Where it is tested

- `regnmed-core/src/hash.rs` — golden digests for v1 **and** v2;
  party reassignment changes the v2 hash; v1/v2 never collide.
- `regnmed-api/tests/grupper/reskontro.rs` (real Postgres, also CI) — the whole
  flow over HTTP: flag account, create customer, party-requirement
  enforcement both ways, invoice + partial payment, mixed-version chain
  verification, spesifikasjon saldo, åpne poster matching with
  over-match rejection (in Norwegian), SAF-T with customers. The
  overview lists the party with items on both sides and ties 1500 to
  the hovedbok; the match history names the match; undoing it reopens
  both sides.
- `regnmed-api/tests/grupper/migrering.rs` — imported supplier items
  keep due date and a valid KID as payable terms (seen in
  `payments/payable`), a bad KID is named and not stored, and the
  preview returns the rows as read.
- `ui/portal/src/sections/parties/matching.test.js` — suggestions:
  exact amounts whatever the order, oldest first for the rest, nothing
  suggested twice.
