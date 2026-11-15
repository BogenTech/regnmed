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

Registeret kontrollerer feltene før databasen ser dem, og svarer på
norsk med hva som må rettes: nummer og navn må fylles ut, salgsprisen
kan ikke være negativ, en ukjent mva-kode navngis, og et nummer som alt
er i bruk sies fra om som det (unik-bruddet gjenkjennes på SQLSTATE
23505, ikke på en gjetning). **Inntektskontoen må være et firesifret
nummer i klasse 3** (3000–3999): en produktlinje er et salg, og en konto
utenfor inntektsklassen ble før godtatt i registeret og feilet først på
fakturaen, langt fra der feilen ble gjort. Regelen gjelder bare verdier
som skrives — eksisterende produkter røres ikke.

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

`regnmed-core::inventory` is a pure fold over the movements in
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

**Forhåndsvisning.** Med `?preview=true` kjører tellingen nøyaktig samme
beregning i samme transaksjon og ruller den tilbake: svaret bærer
justeringene (bokført mot talt antall), verdien etter tellingen,
bokført saldo på lagerkontoen og `endring_ore` — beløpet bilaget vil
føre. Portalen viser dette og ber om bekreftelse før noe bokføres, så
det personen godkjenner er det som blir bokført. Talt antall kan være
null, men ikke negativt.

Bevegelsene har samme regler som migrasjon 0024s sjekker, men sjekket
først og forklart: et varekjøp må ha positivt antall, en justering må ha
et notat, antallet kan ikke være null og kostprisen ikke negativ.
`GET …/inventory` is both the lagerstatus and the telleliste (nummer,
navn, beholdning, snittkost, verdi per product).

## Endpoints

- `GET/POST /companies/{id}/products`, `PUT …/products/{nummer}`
  (write requires bokforing; nummer immutable)
- `GET /companies/{id}/inventory` — status/telleliste
- `GET/POST /companies/{id}/inventory/movements` (`?produkt=` on GET;
  POST accepts kjop/justering only)
- `POST /companies/{id}/inventory/count` — varetelling

Portal: **Produkter**-seksjonen. Registeret er en stablet liste som
passer en telefon, med søk når listen blir lang og deaktiverte produkter
bak «Vis deaktiverte». «Endre» åpner et skjema under raden for navn,
pris, mva, inntektskonto (kontovelgeren fra Hovedbok, begrenset til
klasse 3) og lagerføring, og sender bare det som er endret. En tom pris
blir aldri 0 kr. Varelageret viser beholdning, snittkost og verdi per
produkt; «Bevegelser» åpnes under raden. Varetellingen er to steg:
fyll inn talt antall, se over forskjellene og beløpet, og bokfør.
Skriveknappene vises bare med `PRODUKT_SKRIV`/`LAGER_SKRIV`, og
varelageret hentes bare med `LAGER_LES` — uten den får man fortsatt
registeret. Produktvelgere på faktura og tilbud fyller pris og mva
med mindre de overstyres.

## Where it is tested

- `regnmed-core/src/inventory.rs` — valuation unit tests (rounding, count
  at average, oversell, exact depletion).
- `crates/regnmed-api/tests/grupper/products.rs` — end to end: copy-at-issue
  (register edits and deactivation vs issued documents), automatic
  salg/retur movements through faktura, kreditnota and
  tilbud→ordre→faktura, DB-layer immutability of movements and nummer,
  varetelling posting against the booked 1460 saldo, en forhåndsvisning
  som ikke etterlater noe, de norske feilmeldingene for register og
  bevegelser, chain verification over the lot.
- `ui/portal/src/sections/produkter/produkt.test.js` — prisen (tom er
  ikke 0), inntektskontoen, antall, søk og tellingslinjene.
