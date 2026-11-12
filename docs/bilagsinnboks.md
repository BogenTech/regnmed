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

Forfallsdato og KID fra leverandørens faktura følger med i samme
transaksjon (`due_date`, `kid`) og lagres ved leverandørposten — se
[betaling.md](betaling.md). Portalens bokføringsskjema fyller dem fra
forslaget når det har funnet en leverandør.

With attestering active ([attestering.md](attestering.md), #47) the
same transaction first demands a governing `godkjent` decision on the
document — and refuses when the bokfører is the person who attested
it. The check lives inside the transaction, so it cannot be bypassed
by calling the API directly.

## Endpoints

| Endpoint | Access |
| --- | --- |
| `POST /companies/{id}/inbox?filename=` (bytes) | admin/bokforing |
| `GET /companies/{id}/inbox[?status=ny\|behandlet&limit=]` | any (revisor reads) |
| `GET /companies/{id}/inbox/{doc}/content` | any, hash-checked |
| `POST /companies/{id}/inbox/{doc}/bokfor` (voucher draft) | admin/bokforing |
| `POST /companies/{id}/inbox/{doc}/avvis` (`{note}`) | admin/bokforing |
| `POST /companies/{id}/inbox/{doc}/attester` (`{godkjent, note?}`) | admin/bokforing |
| `GET /companies/{id}/inbox/{doc}/attestering` | any (revisor reads) |

I portalen er innboksen Bilag-seksjonens forside: kamera, flere filer
om gangen eller dra-og-slipp, e-post-inn-adressen med kopieringsknapp,
og listen over det som venter (med attesteringsstatus når regelen er
på). Hvert dokument åpnes på sin egen side (`#/c/{id}/bilag/{doc}`) med
dokumentet vist ved siden av bokføringen (PDF og bilder; se
docs/auth.md om hvordan), forslaget fylt inn med en gang, attestering
og avvisning på samme sted, og etter bokføring rett videre til neste
dokument som venter.

Bokføringsskjemaet spør i den ENKLE formen om det man ser på
dokumentet — hvordan det ble betalt (faktura som skal betales / betalt
fra bank / kontant), leverandør, beløp inkl. mva, mva-sats og hva det
var (kontovelgeren) — og viser linjene det vil bokføre: kostnad netto,
inngående mva på 2710 og brutto mot 2400 med leverandøren som part
(eller mot 1920/1900). Mva-delingen speiler `regnmed-core::vat::
split_gross` (`bookingMath.js`, testet). «Avansert» gir linjene
direkte, med leverandørvalg per linje og om debet og kredit går opp.
Server er fortsatt den som validerer og bokfører; skjemaet sier fra
FØR innsending om en konto ikke finnes i kontoplanen eller
standardkatalogen, og legger en standardkonto inn i kontoplanen første
gang den brukes. Behandlede dokumenter, bokførte bilag (søkbare, i
sider, med vedlegg) og innstillingene (attesteringsregelen og
e-post-inn) er egne faner.

## Deliberately not yet

OCR/tolkning of uploaded documents (suggesting account and amount) is a
later enhancement; it must only ever *suggest* — the accountant decides,
and the posted voucher is what carries legal meaning. E-mail-in and
mobile capture ride on the same endpoint when they come.

## Where it is tested

- `crates/regnmed-api/tests/grupper/innboks.rs` (real Postgres, also in CI):
  upload → list → bokfør posts voucher + attachment + status in one
  transaction (attachment bytes hash-identical to the upload); an
  unbalanced draft fails and leaves the document undecided; re-deciding
  refused; rejection requires a note; revisor can read but not decide;
  outsiders 404; the chain verifies over the new voucher.
