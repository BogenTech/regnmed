# Revisorrollen og verifikasjonsrapporten

The pitch to revisorer is the product's core promise turned into a
workflow: **don't trust us — verify**. This document covers what the
revisor role can do and what the verification report states.

## The role

A revisjonsfirma reaches a client through the same marketplace flow as
regnskapsførere ([marketplace.md](marketplace.md)): verified
autorisasjon (Finanstilsynet, fail-closed), request → accept →
engagement. An engagement of kind `revisjon` resolves to **`les`
access** — the revisor can read everything (reports, reskontro, bilag,
attachments, anchors) and mutate nothing; every write endpoint requires
`bokforing` or `admin`. Ending the engagement revokes access
immediately (valid_to is exclusive — [auth.md](auth.md)).

## The verification report

`GET /companies/{id}/reports/revisjon` (portal: Rapporter → Revisjon)
runs every check the system can make about its own ledger and states
the outcome. A failed check becomes an **AVVIK** line in the report —
it is never an error that hides the document.

| Kontroll | What it proves |
| --- | --- |
| Hash-kjede fra genesis | every voucher re-hashed from stored content; links and chain head intact ([ledger.md](ledger.md)) |
| Bilagsvedlegg | attachment bytes re-hashed against stored SHA-256 ([perioder.md](perioder.md)) |
| Ekstern forankring | anchored heads still on the live chain; stored roots recompute from their leaves ([anchoring.md](anchoring.md)) |
| Reskontro mot hovedbok | Σ reskontro = kontosaldo, konto for konto — se avsnittet under |
| Balansekontroll | all postings sum to exactly zero øre |
| Periodelåsing | current lock and the size of the insert-only lock history (informational) |
| Regelverkssatser | no monitored sats domain in the satsregister is older than its known change cadence ([regelverk.md](regelverk.md)) — outdated satser would silently produce unlawful gebyrer/renter |
| Balansedokumentasjon | which balance accounts with a nonzero saldo lack documentation at the latest closed period ([bokføringsloven §11](https://lovdata.no/lov/2004-11-19-73/§11), balansedokumentasjon.md). **This one IS an avvik**, unlike Dokumentasjon below: §10 says booked information shall be documented and the documentation may lawfully live elsewhere, while §11 says it shall exist for the balance post — and regnmed is where the company records that it does. Also reports accounts posted to AFTER they were reconciled, which is a different finding and said in different words |
| Dokumentasjon | how many bilag lack an attachment in regnmed, oldest first ([bokføringsloven §10](https://lovdata.no/lov/2004-11-19-73/§10)). **Informational, never an avvik**: a missing attachment is not proof of a missing document — it may live in another oppbevaringsmedium, and documentation legitimately arrives after the posting. Bilag that carry documentation BY CONSTRUCTION are not counted (faktura and innboks copy the document onto the voucher when it is posted, so they simply have one), and the import journal is left out because kontroll «Importert historikk» hashes its source files instead. The same set backs `GET …/vouchers?uten_vedlegg=true`, so whoever tidies up works from exactly the numbers the revisor read |
| Importert historikk | which external SAF-T files the ledger was built from ([migration.md](migration.md)): full content hashes from the insert-only import log, so the source system's export can be compared byte for byte; history imported before the log existed is stated, never hidden (informational) |

The report also lists every external anchor covering the company
(timestamp, sequence, root hash, witnesses) and the chain head at
generation time.

## Kontroll 4: reskontroavstemmingen

Reskontroen er ikke et annet regnskap — det er de samme posteringene
sett per part ([reskontro.md](reskontro.md)). Derfor er avstemmingen en
likhet som må holde **konto for konto**:

> summen av partenes poster på kontoen = kontoens egen saldo i
> hovedboken

Begge sider er rene `SUM(amount_ore)`-spørringer over de samme
posteringene; ingen av dem er lagret tilstand. Likheten kan bare briste
på tre måter, og alle tre kontrolleres:

| Funn | Hva som er galt |
| --- | --- |
| **Differanse** | en postering på en reskontrokonto uten part: beløpet er i kontoens saldo, men i ingens spesifikasjon. Avviket oppgir hovedbok, reskontro og **differansen i øre**, ikke bare et antall. Partsløse posteringer som tilfeldigvis summerer til null rapporteres også — antallet er feilen, ikke summen |
| **Part av feil type** | en kunde bokført på en leverandørkonto (eller omvendt): beløpet havner i den spesifikasjonen kontoen ikke er |
| **Part utenfor reskontrokonto** | en konto som *ikke* er merket, men som bærer partsposteringer: beløpet står i partens saldo mens ingen reskontrokonto i hovedboken holder det |

De to siste er nådd uten at noe i hovedboken er endret — hovedboken er
append-only. Det er **flagget** som flytter seg: `reskontro_kind`
nullstilles av åpningsbalansen og SAF-T-importen (de har ingen
partfordeling ennå — [migration.md](migration.md)), settes tilbake av
åpne-poster-importen, og kan endres for hånd etterpå. En kontroll som
bare ser på kontoer som er merket *nå*, ser derfor ikke beløp som
importen la igjen på utsiden.

Posteringsveien selv nekter alle tre tilstandene (`post_voucher` krever
part av riktig type på merket konto og nekter part på umerket konto) —
kontrollen beviser at det faktisk holder i dataene, i stedet for å anta
det.

Hvert funn er en egen AVVIK-linje, navngir kontoen og beløpet, og
rendres på sin egen linje i tekstrapporten.

`?format=tekst` downloads a **deterministic plain-text rendering**
(`regnmed-core::revisjon::render_text` — same input, same bytes) meant
for the revisor's own archive, ending with the independent
re-verification procedure: re-walk the chain from the documented
format, compare roots against the public `/anchors` feed and one's own
copies, verify RFC 3161 tokens offline with `openssl ts`, and hash the
source system's SAF-T files against the import log.

Any access level may generate the report — verification never mutates —
and no access yields 404, as everywhere.

## Where it is tested

- `crates/regnmed-core/src/revisjon.rs` — deterministic rendering, the
  verdict flip on a failed kontroll, "no anchors" stated not hidden, one
  line per finding, and the reskontro reconciliation itself: the
  difference in øre, party-less postings that net to zero, a party of
  the wrong kind, a party on an unflagged account, and a flagged account
  with no postings at all (zero ties out).
- `crates/regnmed-api/tests/grupper/revisjon.rs` (real Postgres, also in CI) —
  a revisor whose only path is a `revisjon` engagement generates the
  report; every kontroll passes on a healthy ledger (reskontro,
  period lock and anchor present; "ingen importert historikk" stated);
  a planted anchor mismatch flips `alle_ok` and marks Ekstern
  forankring AVVIK; the text download renders with the verdict;
  outsiders get 404. A second test drives all three reskontro findings
  through the flag (set, cleared, set again — the åpningsbalanse path)
  and asserts the konto, the amount and the øre difference by name,
  that only kontroll 4 fails, and that a healthy company reports the
  total it reconciled.
- `crates/regnmed-api/tests/grupper/saft_migration.rs` — a ledger built
  from four import files carries kontroll «Importert historikk» with
  the full sha256 of each file in the report.

## Hva revisor ser av lønn

Et `revisjon`-oppdrag gir rollen **`revisor`**: lesing som en intern
leser, pluss `LONN_LES` og `LONNSSLIPP_LES_ALLE` (#55, docs/auth.md).

Det er et uttrykkelig valg, ikke en bieffekt. Lønn er revisjonspliktig —
en vesentlig kostnad, med forskuddstrekk og arbeidsgiveravgift som
lovpålagte størrelser — så revisor må kunne kontrollere både
ansattregisteret og den enkelte lønnsslippen. Fram til #55 fulgte det
bare av at revisor og en intern leser var samme rolle; nå er de to
skilt, og det er den interne leseren som har mistet tilgangen.

Revisor er fortsatt skrivebeskyttet. Lønnstilgangen er lesing, ikke en
oppgradering — det har sin egen test.
