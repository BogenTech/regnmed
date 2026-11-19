# Bank reconciliation

Reconciliation proves that the ledger's bank account and the bank's own
records agree — bokføringsforskriften expects bankavstemming as part of
ajourhold, and unexplained differences are the classic audit finding.

## Connectivity tiers (how statements reach regnmed)

| Tier | Mechanism | Status |
| --- | --- | --- |
| 1. File upload | **camt.053** (ISO 20022) exported from any Norwegian nettbank — no bank agreement needed | **Implemented** |
| 1b. CSV upload | Bank CSV exports, layout auto-detected from headers | **Implemented** |
| 2. PSD2 / open banking | Account feeds through an aggregator (Enable Banking) — the add-on «Bankkobling» | **Built, not switched on** — see «Bankkobling» below; needs a registered application and, before customers, a contract |
| 3. Direct filutveksling | SFTP/ISO 20022 agreements per bank or via Mastercard Payment Services (also OCR-giro, issue #16) | Later — per-customer onboarding |

All tiers feed the **same** reconciliation engine; only the transport
differs. A company card's statement is imported the same way, on the
card's own ledger account — also a credit card account in class 2,
which the account list then includes (docs/utlegg.md «Firmakort»). Building file-first was deliberate: it works for every customer
on day one with zero agreements.

## Bankkobling (tier 2, migration 0096, #70)

Kontoutdrag hentet fra banken i stedet for lastet opp. Det er TRANSPORT rundt
den samme motoren: det som kommer inn blir et vanlig `bank_statement` og
matches som en fil.

**Leverandør: Enable Banking.** Aggregatoren holder eIDAS-sertifikatene; vi har
en applikasjons-id og en RSA-nøkkel og signerer en kort JWT per kall
(`regnmed-gov::enablebanking`). Valgt fordi den har sandkasse og lar oss teste
mot egne kontoer i produksjon før kontrakt; GoCardless/Nordigen tar ikke nye
kunder, og Neonomics/Tink krever avtale først. Klienten er én modul, så et bytte
er avgrenset. **Feltnavnene følger den publiserte API-referansen (lest
2026-11-19) og er IKKE prøvd mot ekte trafikk ennå** — første ekte økt skal
fanges og legges i testene (Stripe-lærdommen, CLAUDE.md punkt 56).

**Tre regler, alle testet og sett feile:**

1. *Uten konfigurert leverandør finnes ikke funksjonen.* `ENABLEBANKING_APP_ID`
   og `ENABLEBANKING_KEY_FILE` (begge eller ingen; nøkkelen er en fil utenfor
   repo og image, docs/secrets.md). Uten dem svarer status `konfigurert: false`,
   portalen viser ikke kortet, og hvert kall nekter.
2. *Uten tillegget kan selskapet verken koble til eller hente.* `company_addon`
   er dekningsrader per tillegg (som `abonnement`): tildeling er en innsetting,
   avslutning setter `valid_to`. Raden skrives foreløpig av systemadmin i
   plattformkonsollen (`POST /platform/companies/{id}/addons`, med referanse);
   kjøp i portalen og fakturering av tillegget er ikke bygget.
3. *Kontoen banken gir tilgang til må være den brukeren oppga.* Brukeren oppgir
   kontonummeret (standard: selskapets eget fra Firmaopplysninger) før de sendes
   til banken; ved retur må økten inneholde nettopp den kontoen (IBAN eller
   BBAN), i NOK. Ellers lukkes økten hos leverandøren og ingenting kobles.

**Flyten.** `connect` skriver en ventende `bank_connection` (id-en er `state`) og
gir en URL hos banken, der brukeren godkjenner med BankID. Banken sender
nettleseren til `/bank/callback`, som er åpen av nødvendighet og derfor ikke gjør
annet enn å gi parameterne videre til portalen; den innloggede brukeren fullfører
med `complete`. Vi lagrer leverandørens øktreferanse, aldri bankens
påloggingsdata. Samtykket varer det banken tillater, høyst 180 dager, og må så
fornyes: å koble til en konto som alt har en kobling ERSTATTER den i samme
transaksjon (aldri to strømmer inn i én konto).

**Henting.** Bare bokførte transaksjoner (`BOOK`; ventende hoppes over som PDNG i
camt.053). En transaksjon som ikke kan leses feiler hele hentingen: en halv
import ville skjult en manglende linje. Hver transaksjon får en nøkkel
(`regnmed-core::bankfeed::dedupe_keys`: bankens referanse, ellers hash av
innholdet pluss ordenstall blant like transaksjoner den dagen), og nøklene
skrives i `bank_feed_seen` per HOVEDBOKSKONTO i samme transaksjon som utdraget.
Vinduene overlapper med vilje (banker bokfører sent), og dette er det som hindrer
at overlapp blir en kopi. Første henting starter dagen etter nyeste linje som
alt ligger på kontoen (opplastede filer hentes ikke om igjen), ellers 90 dager
tilbake. Sier banken at samtykket er borte, avsluttes koblingen med beskjed om å
koble til på nytt; andre feil noteres på raden og neste henting prøver igjen.

**Ikke bygget:** nattlig henting (bare «Hent nå» i portalen; en planlagt jobb
kommer når en applikasjon er registrert og nøkkelen ligger i clusteret), varsel
før samtykket utløper, kjøp og fakturering av tillegget, og saldo fra banken
(utdraget har ingen inn-/utgående saldo, som CSV). **Kjent begrensning:** laster
noen opp en fil for dager koblingen alt har hentet, kommer de linjene to ganger.

## Data model (migration 0007)

- `bank_statement` — one imported statement per ledger bank account.
  Statements are dokumentasjon: insert-only for the app role, and
  re-import of the same bank statement id (`Stmt/Id`) is rejected —
  imports are idempotent.
- `bank_transaction` — statement lines. Amounts are stored in **ledger
  sign for the bank account**: money in (CRDT) = debit = positive, so a
  bank transaction equals its ledger entry exactly.
- `bank_match` — links one bank transaction to one ledger entry
  (`method` auto/manual, who, when). Unique on both sides.
  **"Unmatched" is always computed as the absence of a match row** —
  never stored mutable state, same philosophy as balances.

## The pieces

- **camt.053 parser** (`regnmed-core::camt053`, pure): tolerant,
  version-agnostic (`camt.053.001.0x`), reads only what reconciliation
  needs (statement id, IBAN, OPBD/CLBD balances, booked entries with
  date/amount/direction/reference/description). Pending (`PDNG`) entries
  are skipped. Amounts parse to integer øre.
- **CSV parser** (`regnmed-core::bankcsv`, pure): tier 1b. Detects the
  layout from the header row instead of maintaining per-bank profiles —
  delimiter (`;`/tab/`,`), date column ("dato"/"bokføringsdato"/…,
  never "rentedato"), one signed beløp column or separate inn/ut
  columns, Norwegian number formats ("1 234,56"), optional
  KID/referanse. A file it cannot understand fails loudly with the
  headers it saw — never a silent half-import. Output is the same
  statement shape as camt.053, so storage, matching and reconciliation
  are one engine; the statement ref is the file's content hash, so
  re-import stays idempotent, and balances are absent (a CSV has none —
  shown as absent, never zero). Same endpoint: the import
  distinguishes XML from CSV by content.
- **Matching engine** (`regnmed-core::bank`, pure, deterministic):
  equal amount + same day first, then equal amount within a ±5-day
  window. Each side is used at most once. **Ties are never guessed** —
  two equally close candidates fall to manual review. KID/reference
  matching arrives with reskontro (entries don't carry payment
  references yet).
- **Forslag, aldri gjetting** (`regnmed-core::bank::suggest_entries`):
  for hver bankpost motoren lot ligge, inntil tre åpne poster med
  NØYAKTIG samme beløp (fortegn inkludert), nærmeste dato først — og
  hver post foreslås for bare ÉN bankpost (den nærmeste), som i
  auto-matchingen, så tre like gebyrer og ett bilag ikke gir tre like
  knapper. Et
  annet beløp foreslås aldri — og en manuell kobling av ulike beløp
  avvises i databasen-laget (`manual_match`) med beløpene navngitt, så
  en feilkobling ikke kan skje med ett klikk.
- **Bokfør fra banklinjen**: en bankpost uten bilag bak seg (gebyr,
  renter) bokføres og kobles i ÉN transaksjon
  (`post_from_bank_line`): bankkontoen mot valgt konto på
  bokføringsdatoen, uten mva, med kontoutskriftens referanse og tekst
  på banklinjen som dokumentasjon (referanse, bokføringsdato, beløp og
  tekst står på bilagslinjen; kontoutskriftsradene er innsettings-bare
  og oppbevares). Samme attesteringsport som manuelt
  bilag (`refuse_if_attestation_required`, docs/attestering.md) — med
  aktiv policy over grensen må det gå via innboksen. Krever både
  `BANK_AVSTEM` og `BILAG_BOKFOR`. Har utgiften mva eller kvittering,
  hører den hjemme i innboksen.
- **Differansen regnes på kontoutskriftens dato**: hovedboken summeres
  til og med siste kontoutskrifts sluttdato
  (`ledger_at_statement_ore`) og sammenlignes med bankens utgående
  saldo. Dagens saldo ville tatt med alt bokført etterpå. CSV har ingen
  saldo, og differansen vises da som ukjent, aldri som null.
- **Åpne poster i hovedboken** vises bare innenfor perioden
  kontoutskriftene dekker (en måned på hver side): historikk fra før
  første kontoutskrift er ikke avstemmingsarbeid, og ville ellers blitt
  hele den importerte hovedboken.
- **Web API** (engagement-guarded; revisor 'les' may read, importing and
  matching require bokforing/admin):
  - `POST /companies/{id}/bank/statements?account=1920` — camt.053 XML
    body; imports and auto-matches, returns counts.
  - `GET /companies/{id}/bank/accounts` — selskapets bankkontoer
    (aktive 19xx) med saldo, siste kontoutskrift og antall uavstemte.
  - `GET /companies/{id}/bank/reconciliation?account=1920` — ledger
    balance (today and on the statement's end date) vs latest statement
    closing balance, matched count, unmatched bank lines WITH their
    `suggestions`, open ledger entries within the statements' period,
    the statements, and the 50 newest matches (`recent_matches`).
  - `GET /companies/{id}/bank/entries?account=1920&q=` — søk i åpne
    poster på kontoen: bilagsnummer («2026-14»), tekst, eller beløp
    slik det skrives («1 290,00»; `regnmed-core::money::parse_kroner`,
    samme regler som bank-CSV-en). Maks 20.
  - `POST /companies/{id}/bank/transactions/{tid}/post` `{konto, tekst}`
    — bokfør banklinjen og koble den.
  - `POST /companies/{id}/bank/matches` + `DELETE
    /companies/{id}/bank/matches/{bank_transaction_id}` — manual
    match/unmatch; cross-company and cross-account pairings are rejected
    in the database layer.

## OCR giro (KID-innbetalinger)

OCR files from Mastercard Payment Services (tidligere Nets/BBS) carry
KID-tagged incoming payments — the detail behind the lump-sum "OCR
innbetaling" line on the bank statement.

- **Parser** (`regnmed-core::ocr`, pure): the official fixed-width
  80-character record format (service 09), layouts verified against the
  published systemspesifikasjon and the netsgiro reference
  implementation. Control records are enforced (transaction count and
  sum must match, so truncated or tampered files are rejected);
  reversal signs are honored; **invalid KIDs are flagged, never
  rejected** — the bank accepted the payment, so it must be recorded.
- **KID check digits** (`regnmed-core::kid`): MOD10 and MOD11
  validation, plus check-digit generation for faktura (issue #13).
- **Storage** (migration 0008): batches insert-only and idempotent per
  (forsendelse, oppdrag) — a re-uploaded file errors instead of
  duplicating payments.
- **Web API**: `POST /companies/{id}/ocr/files?account=1920` (file
  body), `GET /companies/{id}/ocr/payments?from=&to=`. Same access
  rules as bank reconciliation.
- With **reskontro** (issue #3), payments auto-apply to open invoices by
  KID; until then the payment list is the working view, and OCR batch
  sums reconcile against the statement's lump-sum line.

## Where it is tested

- `regnmed-core/src/camt053.rs` — parsing (balances, signs, pending
  skipped, entities, øre), malformed input.
- `regnmed-core/src/bank.rs` — matching rules, window, no-reuse,
  ambiguity-goes-to-manual.
- `regnmed-api/tests/grupper/bank.rs` (real Postgres, also CI) — the whole flow
  over HTTP: revisor 403 on import, import + auto-match, duplicate
  import rejected, revisor reads reconciliation, manual match to full
  reconciliation, unmatch, stranger 404.
  `a_bank_line_is_posted_from_the_statement_and_amounts_must_agree`:
  bankkontolisten, ingen forslag med annet beløp, kontoutskriftslisten,
  hovedboken på kontoutskriftens dato, søk på beløp som skrevet,
  kobling av ulike beløp avvist, forslaget er posten med likt beløp,
  revisor 403 på bokføring, attesteringsporten, bokført og koblet i ett
  steg (og ikke to ganger), avstemt på øret, kjedeverifikasjon.

## Portal

**Bank** (`#/c/{id}/bank/…`) har fanene Avstemming (forsiden),
Betalinger og Valutakurser. Avstemming: bankkontovelger når selskapet
har flere, «Last opp kontoutskrift» (eller dra filen inn), statuslinje
med banken og hovedboken på samme dato og differansen, og hver
uavstemte bankpost med det som kan lukke den — ett klikk på forslaget
med likt beløp, «Bokfør» (konto foreslått: 7700 for penger ut, 8050 for
penger inn — standardkontoplanens, brukerens valg), eller «Finn
bilag» (søk; poster med annet beløp kan ikke kobles). Under: åpne
poster i hovedboken uten bankpost, de koblede med «Fjern kobling»
(bekreftes), og kontoutskriftene. Knappene følger rettighetene.
