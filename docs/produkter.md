# Produktregister og enkelt varelager

Issue #39. Products make invoicing fast and consistent; the opt-in
varelager covers vareførende SMB with the same discipline as the rest
of the system: **beholdning is never stored — it is a SUM over an
insert-only movement log**, exactly like account balances.

## Registeret

`product` (migration 0024): nummer, navn, salgspris (øre), mva-kode,
inntektskonto, aktiv, lagerført. The register is **editable master
data** — with two deliberate restrictions enforced in the database
(trigger + column grants, like the dimension registry):

- **Nummer is permanent.** It appears on documents and in the movement
  log; renaming it would rewrite history's meaning.
- **Products are never deleted**, only deactivated. Issued lines and
  movements reference them forever; a deactivated product is refused on
  new documents but keeps its history.

## Kopiering ved utstedelse

Document lines (faktura, tilbud/ordre, repeterende maler) may reference
a product, but they **store their own copy** of description, price,
konto and mva-kode, resolved when the line is written
(`regnmed_db::product::resolve_product_line`). Changing the register
therefore never changes an issued document — the reference
(`product_id` on the line) exists only for lager and traceability. The
caller can override any copied value per line (e.g. a negotiated
price); everything left out comes from the register.

One shared line shape serves all three document APIs
(`regnmed-api::product::DocLineRequest`): free-text lines require
description + price; product lines require only `produkt`.

## Varelageret

`inventory_movement` is **insert-only** (append-only triggers +
INSERT/SELECT-only grants). Three kinds:

- `kjop` — varekjøp, positive quantity, carries anskaffelseskost per
  unit. Registered manually (`POST …/inventory/movements`).
- `salg` — inserted **by the invoice transaction itself** for every
  line referencing a lagerført product: quantity is the line quantity
  negated, so a kreditnota line (negative quantity) returns the stock.
  Stock and ledger commit or roll back together; the movement links to
  its invoice. Never registered manually.
- `justering` — manual correction or varetelling; a note is required
  (DB check).

Quantities are integer **milli-units** (1000 = one unit), matching
invoice line quantities. Beholdning per product =
`SUM(antall_milli)`.

### Verdsettelse: gjennomsnittsmetoden

`regnmed-core::lager` is a pure fold over the movements in
chronological order, integer øre, half-away-from-zero rounding per
movement (unit tested, deterministic anywhere it runs):

- Inbound with kostpris → in at that cost.
- Inbound without kostpris (varetelling opp, kreditnota-retur) → in at
  the running gjennomsnittskost (0 if the stock was empty).
- Outbound → out at the running gjennomsnittskost, proportionally —
  emptying the stock exactly removes the value exactly, no residual
  øre.
- Outbound beyond the stock removes the whole value and lets the
  quantity go negative: a negative beholdning is a counting
  discrepancy that must be visible, never hidden.

FIFO and other kostprismetoder are deliberately out of scope (the
issue: gjennomsnitt only; FIFO later if a customer asks).

## Varetelling

Bokføringsforskriften expects a varetelling valued at
anskaffelseskost. `POST …/inventory/count` takes the counted
quantities and, in ONE transaction:

1. inserts a `justering` movement per product whose count differs from
   the computed beholdning (note "Varetelling <dato>");
2. computes the total inventory value after the adjustments
   (gjennomsnittsmetoden over every lagerført product);
3. when posting is requested (default), posts the difference between
   that value and the **booked saldo** on the lager account as an
   ordinary voucher — debit 1460 Varelager / credit 4390
   Beholdningsendring when the value grew (accounts and journal
   overridable). Periodic method: purchases are expensed when bought;
   the telling trues up the balance.

Counting the same numbers twice is a no-op — no movements, no voucher.
`GET …/inventory` is both the lagerstatus and the telleliste (nummer,
navn, beholdning, snittkost, verdi per product).

## Endpoints

- `GET/POST /companies/{id}/products`, `PUT …/products/{nummer}`
  (write requires bokforing; nummer immutable)
- `GET /companies/{id}/inventory` — status/telleliste
- `GET/POST /companies/{id}/inventory/movements` (`?produkt=` on GET;
  POST accepts kjop/justering only)
- `POST /companies/{id}/inventory/count` — varetelling

Portal: **Produkter** section (register, varelager with movement form,
varetelling with inline "talt" inputs, movement history per product);
produkt pickers on the faktura and tilbud forms (choosing a product
supplies price and mva unless overridden).

## Where it is tested

- `regnmed-core/src/lager.rs` — valuation unit tests (rounding, count
  at average, oversell, exact depletion).
- `crates/regnmed-api/tests/products.rs` — end to end: copy-at-issue
  (register edits and deactivation vs issued documents), automatic
  salg/retur movements through faktura, kreditnota and
  tilbud→ordre→faktura, DB-layer immutability of movements and nummer,
  varetelling posting against the booked 1460 saldo, chain
  verification over the lot.
