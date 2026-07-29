# regnmed — project context

Accounting system (regnskapssystem) for the Norwegian market, written in Rust.
Dual-licensed AGPL-3.0 + commercial (see LICENSE, COMMERCIAL-LICENSE.md).

## Product vision

A portal/marketplace where **regnskapsførere** and **revisorer** offer services
to businesses: accountants bring their client portfolios, businesses find
verified-authorized accountants, and both collaborate on the same live ledger.
Primary market is SMB; large corporations are not ruled out but not the target.
Regnskapsførere are the distribution channel — win one firm, win its clients.

The trust story is the moat: the ledger is tamper-evident and independently
verifiable ("don't trust us — verify"), which is also the pitch to revisorer.

## Architecture decisions (do not silently revisit)

- **PostgreSQL 18+** (never below 18) + **sqlx**. Queries use sqlx's runtime
  API for now so builds don't need a live DB; move hot paths to `sqlx::query!`
  + `cargo sqlx prepare` once CI has a database fixture.
- **Money is integer øre (`Ore(i64)`), never floats.** Positive = debit,
  negative = credit. Balances are always `SUM(amount_ore)` — never stored
  mutable state.
- **The ledger is append-only**, enforced in three layers:
  1. Domain: corrections are reversing vouchers (`reverses_voucher_id`), never edits.
  2. Database: append-only triggers reject UPDATE/DELETE/TRUNCATE on
     voucher/entry; the app role (`regnmed_app`) is only granted INSERT/SELECT
     on ledger tables.
  3. Crypto: every voucher stores `hash = SHA-256(prev_hash || canonical content)`
     (canonical netstring serialization in `regnmed-core::hash`; timestamps
     truncated to microseconds so they round-trip through Postgres).
     `regnmed verify-ledger` re-walks chains from genesis. External
     anchoring (docs/anchoring.md): nightly Merkle snapshots of all chain
     heads, root published on the public `/anchors` endpoint and optionally
     witnessed via RFC 3161 (`ANCHOR_TSA_URL`) — DBA-level rewrites become
     provable, not just suspectable.
- **Gap-free voucher numbering** per journal + fiscal year via a counter row
  locked in the posting transaction (sequences can leave gaps).
- **Migrations are append-only in git.** sqlx checksums applied migrations and
  refuses to run if an applied file changed. Never edit an applied migration.
- **Identity: OIDC relying party only.** The IdP is **regnid** (sibling
  repo `../regnid`) — our Rust port of
  [networco-id](https://github.com/networco/networco-id) (C#, sibling
  `../networco-id`, stays the behavioral reference and keeps serving until
  regnid passes the OIDC conformance suite; see regnid's CLAUDE.md for
  parity/hardening checklists). Keep IdPs in their own repos — never vendor
  into this one. regnmed validates tokens against a configured issuer/JWKS
  and must never bake in IdP specifics; the token proves identity only.
  Cross-service SSO verified 2026-07-16: regnid-issued token → regnmed
  `/me`.
- **Authorization lives in regnmed's DB, not in tokens.** Model:
  person → firm membership → **engagement (oppdrag)** → company. Engagements
  (regnskapsfører/revisor ↔ client company, with scope and validity) are
  first-class domain objects — an accountant with 60 clients can't carry that
  in a JWT. Revisor engagements are read-only + chain verification.
- Registries: onboard companies from Brønnøysund (Enhetsregisteret, orgnr
  lookup); verify professional autorisasjon against Finanstilsynet's register
  before anyone can offer services in the marketplace.

## Workspace layout

- `crates/regnmed-core` — pure domain: money, vouchers, double-entry
  validation, canonical chain hashing. **No I/O or DB dependencies, ever** —
  the hash must stay deterministic forever.
- `crates/regnmed-db` — Postgres persistence: migrations
  (`crates/regnmed-db/migrations/`), posting transaction, chain verification.
- `crates/regnmed-api` — HTTP API (axum). Library + thin binary. OIDC RP
  layer in `src/auth.rs` (`Verifier` + `AuthPerson` extractor — add
  `AuthPerson` as a handler argument to protect a route); `/me` resolves
  token → companies + access. Report endpoints in `src/reports.rs`,
  guarded per company via `regnmed_db::company_access` (no access → 404,
  never 403 — don't leak existence). Config: `OIDC_ISSUER`, optional
  `OIDC_AUDIENCE`, `OIDC_JWKS_FILE` (dev/tests: static JWKS, signatures
  still validated), `BIND_ADDR`.
  **API-first principle: the web is the product.** Every user-facing
  capability (reports, exports, later posting and workflows) must be an
  authenticated API endpoint; the CLI wraps the same crate functions for
  ops/admin only and is never the only trigger.
- `crates/regnmed-cli` — `regnmed` binary: `migrate`, `verify-ledger`, `demo`.

## Development

```sh
docker compose up -d      # dev Postgres 18 on port 5433, or:
scripts/dev-db.sh         # same without Docker (brew install postgresql@18)
cp .env.example .env
cargo run -p regnmed-cli -- migrate
cargo run -p regnmed-cli -- demo           # posts vouchers, attempts tampering, verifies chain
cargo run -p regnmed-cli -- verify-ledger
cargo test                                 # unit tests, no DB needed
cargo nextest run --workspace              # hele suiten, ~3,6x raskere enn cargo test
```

Hele suiten kjøres med **cargo-nextest** (også i CI): 48 testbinærer,
de fleste med én test, og `cargo test` kjører binærer etter hverandre —
testene venter på Postgres, ikke CPU. Installer: `cargo install
cargo-nextest --locked` eller `curl -sSLf https://get.nexte.st/latest/mac
| tar zxf - -C ~/.cargo/bin`.

### Language policy (agreed 2026-07-29)

**Prose in the codebase is English. Domain nouns are Norwegian.**

| Where | Language | Why |
| --- | --- | --- |
| Comments, doc-comments | **English** | They explain *why* to whoever maintains this next. None of that reasoning is Norwegian-specific. |
| Test function names | **English** | A test name is a sentence stating what must not break — same audience as a comment. |
| Commit messages | **English** | Same. |
| Domain nouns as identifiers | **Norwegian** | `bilag`, `hovedbok`, `reskontro`, `oppdrag`, `mva`, `sats`, `kontoplan` are statutory terms. "Voucher" is not quite *bilag*; "subledger" is not quite *reskontro*. The code implements XSDs and code lists that use these words — translating opens a gap between the code and the law it encodes. |
| `docs/` | **Norwegian** | Audit-facing: revisorer and certification processes. See the documentation policy below. |
| User-facing strings | **Norwegian** | Error messages, `Rett::beskrivelse()`, the portal. The users are Norwegian. |

So the normal shape is English sentences about Norwegian nouns:

```rust
/// The role is locked (`for update`) before the rights list is rewritten:
/// otherwise two concurrent edits would each delete the old list and let
/// their own through — the union, i.e. more access than either asked for.
```

This was written down after a review found the repo had **drifted**, not
chosen: ~990 Norwegian comment lines against ~1840 English, `regnmed-api`
almost exactly half and half, early work English and recent work
Norwegian. Mixed is worse than either pure choice — a maintainer can rely
on neither language and has to read both.

Two traps when converting: `matrise.rs`'s `START`/`SLUTT` marker strings
must keep matching docs/auth.md **verbatim**, and applied migrations are
never edited (their comments are part of the append-only record, so
`migrations/*.sql` stays as written).

### Secrets policy (agreed 2026-07-26, docs/secrets.md)

**No secrets in this repo, in any form — not even encrypted.** The rule
in docs/gov.md is absolute, test as well as prod, with no carve-out.
That is affordable because there is almost nothing to protect: `.env` is
localhost config (committed as `.env.example`), and a new machine is set
up with `cp .env.example .env` + `docker compose up -d`.

The one real secret is the Maskinporten test key, in
`~/.config/regnmed/` (paths listed in docs/secrets.md — the location is
recorded, the secret is not). It is deliberately **not** backed up: a
second machine runs `scripts/maskinporten-key.sh` to generate its **own**
keypair and registers that public key on the same client
(`MASKINPORTEN_KID` selects between them), and a lost machine is handled
by deleting its key in Samarbeidsportalen. Note that Maskinporten keys
**expire** — the date is on the December regelverksrevisjon checklist.

### Documentation policy (agreed 2026-07-22)

`docs/` is the audit-facing documentation — what the system guarantees,
where each guarantee is enforced, where it is tested — written for
revisorer, certification processes and new developers (index:
docs/README.md). Every milestone updates the relevant document **in the
same change**, like tests. Vendored authority artifacts (XSDs, code
lists) live under docs/ next to the document that explains them.

### Testing policy (agreed 2026-07-22)

Every important change ships with tests in the same commit — not tests
for everything, tests for what must not break: domain invariants (money,
hashing, double-entry), ledger immutability, auth boundaries, and export
formats. Patterns in use:

- Pure logic: unit tests next to the code (regnmed-core).
- **The golden hash test in `regnmed-core::hash` pins the exact digest of
  the canonical serialization. If it fails, the change breaks chain
  verification of every deployed ledger — the format can only be
  versioned, never edited.**
- DB behavior (posting transaction, append-only triggers, deferred
  balance check, tamper detection, SAF-T loader): integration tests in
  `crates/regnmed-db/tests/` and `crates/regnmed-api/tests/` that skip
  politely when DATABASE_URL is unset, and run for real in CI against a
  postgres:18 service (locally: `scripts/dev-db.sh` + `regnmed migrate`).
- SAF-T output is validated against Skatteetaten's official XSD
  (vendored in `docs/saft/`) with xmllint, in unit tests and CI.
- **API-integrasjonstestene er GRUPPERT** (2026-07-29): filene ligger i
  `crates/regnmed-api/tests/grupper/` og samles av sju binærer i
  `tests/` — salg, regnskap, penger, drift, personal,
  tilgang_og_marked, portal_og_drift. Grunnen er byggetid: hver
  `tests/*.rs` er en egen crate som lenkes for seg, og 33 av dem hadde
  én eneste test. Nye tester legges i en eksisterende gruppe (`#[path]`
  i gruppefila), ikke som ny fil i `tests/` — en ny fil der er en ny
  binær. Modulene bruker `use crate::common::…`; `mod common;` står
  bare i gruppefila. `seed_browser.rs` er med vilje alene, siden den
  kjøres for hånd.

### Local production-like cluster (on demand, 8 GB-friendly)

`scripts/dev-cluster.sh up` gives the full topology in a local k3s
cluster (k3d inside a 2 cpu/2 GB colima VM — the tight budget is a
product principle, see ROADMAP.md): Postgres 18, NATS
JetStream, regnid + mail worker, regnmed-api, Traefik ingress. One
issuer URL everywhere — `http://id.regnmed.localhost` works from the
browser (\*.localhost → 127.0.0.1) and inside pods (CoreDNS rewrite to
Traefik), so the whole SSO flow runs exactly as deployed. Rust is
cross-compiled **on the host** (aarch64-musl, `scripts/build-images.sh`)
and only static binaries enter the ~tiny distroless images — the VM
never compiles. `stop` frees all RAM (state survives), `deploy` rebuilds
+ rolls out after code changes, `status` shows pods and URLs. This is
the integration proving ground; daily coding stays on dev-db.sh +
cargo. Manifests: `deploy/base/` + overlays `deploy/local/` and
`deploy/prod/` (kustomize; prod adds TLS, secrets, verified backups —
docs/deploy.md). Not yet: multi-node, operators — add when a concept
needs them.

## Roadmap (agreed order)

The full phased plan (M1 lovpålagt kjerne → M6 tillit og skala, with the
Norwegian-ecosystem integration strategy: Altinn/Maskinporten,
Skatteetaten, BRREG, EHF/Peppol, bank, and migration from
Tripletex/Fiken/Visma/Conta via SAF-T) lives in **ROADMAP.md**; each item
is a GitHub issue under milestones M1–M6. Summary of agreed order:

1. ✅ Ledger core: append-only hash-chained vouchers, verified end-to-end.
2. ✅ Auth + tenancy: engagement schema (migration 0005: person, firm,
   firm_member, company_member, engagement) + OIDC RP middleware; `/me`
   resolves token → "companies I may act for, and as what". Integration
   tests sign real RS256 tokens against a generated JWKS.
3. ✅ SAF-T Financial v1.30 export: pure renderer in
   `regnmed-core::saft` (official grouping code list embedded; no XML
   library — hand-rolled deterministic writer), loader in
   `regnmed-db::saft`, `regnmed saft-export` CLI. Output validated
   against Skatteetaten's XSD (vendored in `docs/saft/`) in unit tests
   and CI (`.github/workflows/ci.yml` installs xmllint).
4. ✅ MVA codes end-to-end: complete standard code list + dated rates
   (`vat_rate`, basis points, history incl. the covid lav-sats cut) in
   migration 0006; pure termin/beregning logic in `regnmed-core::mva`
   (integer øre only); `regnmed mva-report` prints the mva-spesifikasjon
   per termin with utgående/inngående/netto. SAF-T lines carry the rate
   valid on each voucher's date, not the current rate.
5. ✅ Maskinporten foundation + mva-melding: `crates/regnmed-gov`
   (JWT-grant token provider with cache; validation-API client);
   `regnmed-core::mvamelding` builds schema-valid `mvaMeldingDto` XML
   (whole kroner, payable-positive signs, code 0 excluded; XSD vendored
   in `docs/mva-melding/`); `regnmed mva-melding --validate` runs the
   whole chain. Live validation/submission awaits Maskinporten client
   registration (docs/gov.md, issue #8).
6. ✅ Bank reconciliation (file tier): camt.053 parser
   (`regnmed-core::camt053`, quick-xml — parsing only, our XML output
   stays hand-rolled) + deterministic matching engine
   (`regnmed-core::bank`, ties go to manual, never guessed) + migration
   0007 (statements insert-only; "unmatched" computed, never stored) +
   web endpoints under `/companies/{id}/bank/…` (revisor reads,
   bokforing matches). PSD2/aggregator and CSV tiers documented in
   docs/bank.md as later steps.
7. ✅ OCR-giro: fixed-width record parser (`regnmed-core::ocr`, offsets
   verified against the official spec + netsgiro; control records
   enforced, invalid KIDs flagged not rejected) + MOD10/MOD11 in
   `regnmed-core::kid` (reused by faktura later) + migration 0008
   (idempotent per oppdrag) + `/companies/{id}/ocr/…` endpoints.
8. ✅ Reskontro: party master data + party binding on entries via **hash
   format v2** (version per voucher; v1 history verifies forever; golden
   tests pin both formats — docs/reskontro.md). Reskontro-flagged
   accounts require a party of matching kind, others reject parties
   (enforced in post_voucher). Åpne poster matching with computed
   remainders; SAF-T exports Customers/Suppliers + line party ids;
   `/companies/{id}/parties…` + `/reskontro/matches` endpoints.
9. ✅ Faktura: gap-free invoice numbers atomic with the ledger posting
   (`post_voucher_in` takes the caller's transaction), KID from invoice
   number (MOD10), automatic posting (receivable w/ party, revenue lines
   w/ VAT codes, dated-rate VAT), kreditnota with auto reskontro-match,
   OCR payments resolve their invoice by KID at import. Invoices
   immutable once issued. `/companies/{id}/invoices…` endpoints.
   PDF/EHF rendering arrives with portal + access point (docs/faktura.md).
10. ✅ Periodelåsing + bilagsvedlegg — **M1 (lovpålagt kjerne) complete.**
   Insert-only lock history (reopening = admin-only, audited); posting
   check + DB trigger both enforce; attachments append-only with content
   SHA-256 verified on download and by `verify-ledger` (attachment-set
   chain binding deferred to M6 anchoring — docs/perioder.md).
11. ✅ Portal UI (docs/portal.md): no-framework SPA embedded in
   regnmed-api (include_str!, same origin, no CORS), OIDC code+PKCE
   against regnid with the token exchange proxied via `/auth/token`.
   Theme contract honored: themes.css is a copy of regnid's canonical
   file (update together). Sections: oversikt, faktura, reskontro,
   mva (+ eksport), bank, bilag, periode. Browser-verified end to end.
   **HISTORIKK — erstattet 2026-07-29 av Svelte-portalen (#76, punkt
   under): app.js/theme.js/app.css og scripts/build-css.sh finnes ikke
   lenger; temakontrakten består, filen ligger nå i ui/portal/.**
12. ✅ Marketplace onboarding (docs/marketplace.md): orgnr MOD11 in
   core; BRREG client + fail-closed Finanstilsynet adapter in
   regnmed-gov (both URL-configurable; FT endpoint pinned during pilot);
   `POST /companies` onboards from Enhetsregisteret (registry name,
   creator becomes admin, starter kontoplan seeded, slettet/konkurs
   refused), `POST /firms` gated on verified autorisasjon (moment +
   source recorded). Portal onboarding card, verified against live BRREG.
13. ✅ Directory + engagement flow (docs/marketplace.md): verified-only
   directory, request→decide→engagement in one tx (access live via /me,
   no re-login), end sets valid_to (EXCLUSIVE in access resolution —
   revocation is immediate; changed in tenancy.rs). Portal: Oppdrag
   section per company + Byrå view (requests, clients) for firm members.
14. ✅ SAF-T migration import (docs/migration.md): tolerant parser in
   core (round-trip tested against our own exporter), all-or-nothing
   import into an EMPTY ledger only — history chain-posted through
   post_voucher_in into an IMP journal, opening balances must sum to
   zero, conservative reskontro flagging, warnings surfaced. Portal
   offers it on empty companies. `POST /companies/{id}/import/saft`.
15. ✅ External anchoring (docs/anchoring.md): Merkle snapshot format v1
   in `regnmed-core::anchor` (golden root pinned; leaves sorted by
   company id, CT-style odd promotion), append-only anchor tables
   (migration 0014), RFC 3161 witness client with hand-rolled DER in
   `regnmed-gov::tsa`, public `GET /anchors` feed + per-company
   inclusion proofs + `/anchors/verify`, `regnmed anchor` CLI + nightly
   CronJob in deploy/local, portal Forankring card. `verify-ledger`
   fails on anchor mismatches. Attachment-set binding deferred (leaf v2,
   sketched in docs/anchoring.md).
16. ✅ Lovpålagte spesifikasjoner (docs/rapporter.md, closed the last M1
   issue #4): saldobalanse, kontospesifikasjon (running saldo +
   bilagshenvisning), bokføringsspesifikasjon in chain order —
   `regnmed-db::regnskap`, pure SUM queries; resultat/balanse grouped
   per NS 4102 in `regnmed-core::regnskap` (presentation signs,
   udisponert resultat keeps balansen at zero differanse). Five
   `/reports/*` endpoints + portal Rapporter section.
17. ✅ Revisorens verifikasjonsrapport (docs/revisjon.md, closed #24):
   `GET /companies/{id}/reports/revisjon` runs six kontroller (chain
   re-walk, attachment hashes, anchor consistency, reskontro tie-out,
   balance, period-lock status) — failures become AVVIK lines, never
   hidden errors; `?format=tekst` downloads the deterministic rendering
   (`regnmed-core::revisjon`) ending with the independent
   re-verification procedure. Portal: Rapporter → Revisjon tab.
18. ✅ Bilagsinnboks (docs/bilagsinnboks.md, closed #21 — **M5
   complete**): migration 0015 (content immutable via column grants +
   trigger, decisions one-way ny→bokfort/avvist, nothing deletable);
   `bokfor_inbox_document` posts voucher + copies document into
   `attachment` + marks status in ONE transaction (failed posting
   leaves the document undecided); avvis requires a note. Endpoints
   under `/companies/{id}/inbox…`; portal Bilag section opens with the
   inbox (upload, inline bokfør form, avvis).
19. ✅ Kontoplan wizard + manuell åpningsbalanse (docs/migration.md,
   closed #18): `regnmed-core::kontoplan` suggests NS 4102 mappings
   (identity/truncate/pad/name-match against the vendored standard
   names — heuristics only, the admin decides);
   `POST …/import/saft/analyze` previews, import accepts a JSON
   envelope `{file, mapping}` (apply_mapping rewrites + merges, 4-digit
   targets enforced). `POST …/opening-balance` posts a manual
   Åpningsbalanse (zero-sum, empty ledger only, reskontro flags
   deferred w/ warning). Portal: mapping table + åpningsbalanse card on
   empty companies.
20. ✅ Frugality gate in CI (docs/frugality.md, closed #28):
   `scripts/frugality.sh` builds release, checks binary budgets
   (api 24 MB, cli 20 MB) and regnmed-api **peak RSS under real load**
   (VmHWM, budget 64 MB = the k8s container limit; measured 11/8/11 MB
   2026-07-23). Separate `frugality` CI job; budget raises must be
   conscious commits touching script + deploy limit together.
21. ✅ Bank CSV tier (docs/bank.md, closed #15): `regnmed-core::bankcsv`
   detects layout from headers (delimiter; dato — never rentedato;
   signed beløp or inn/ut pair; norsk tallformat; KID/referanse);
   unknown layouts fail loudly listing the headers seen. Same endpoint
   (XML vs CSV by content), same engine; statement ref = content hash
   (idempotent re-import), balances absent not zero (fixed a latent
   nullable decode in reconciliation_status).
22. ✅ Production deploy scaffolding (docs/deploy.md, closed #27):
   deploy/ restructured to base + local/prod overlays (local render
   proven byte-identical — dev-cluster.sh untouched); prod overlay:
   cert-manager TLS, `db-credentials` secret out-of-band (no credential
   in any rendered manifest), pinned image tags, ANCHOR_TSA_URL on the
   anchor CronJob, nightly pg_dump + **weekly restore-verification**
   (restore into scratch DB + verify-ledger over the restored copy —
   also runnable anywhere via scripts/backup-verify.sh, exercised both
   ways: clean passes, forged anchors fail). PITR via CloudNativePG
   documented as growth path. Beware: strategic-merge env patches need
   `value: null` to remove the base's plaintext value.
23. ✅ Satsregisteret (docs/regelverk.md, closed #49): migration 0016
   `sats` — dated, append-only, every row carries its legal kilde;
   seeded w/ verified 2025–2026 values (forsinkelsesrente,
   standardkompensasjon, inkassosats, purregebyr 1/20, statens
   km-satser, terskler). Pure `sats_on` + `foreldede_domener` in
   `regnmed-core::sats` (cadence table; thresholds exempt); the
   revisjonsrapport gained kontroll 7 "Regelverkssatser" so the yearly
   regelverksrevisjon is machine-verified. Consumers: #29/#40/#42/#46.
24. ✅ Per-inntektsår kodelister (docs/regelverk.md, closed #50):
   `regnmed-core::saft` ARGANGER registry — the næringsspesifikasjon
   list is selected by the exported year (2025-2026 vendored);
   `render`/`grouping_for` are year-aware and FAIL LOUDLY outside
   coverage (test-pinned); wizard suggests from the newest vintage;
   CLI prints the governing årgang. New year = vendor CSV + registry
   entry.
25. ✅ Purring og betalingsoppfølging (docs/purring.md, closed #29):
   pure regelverk in `regnmed-core::purring` — forsinkelsesrente
   segmented across the halvårlige satser (dagen etter forfall, /365,
   per-period rounding so the spesifikasjon sums exactly), stegregler
   (gebyr tidligst 14 dager etter forfall, tak fra satsregisteret per
   sendedato — purregebyr_maks eller standardkompensasjon for
   næringsdrivende, maks to gebyrbelagte skritt, enveis trapp,
   inkassovarsel minst 14 dagers frist + lovtekst; inkasso selv
   overlates til bevillingshavere). Gebyr/rente bokføres som ordinært
   bilag (debet 1500 m/ kunde — nytt åpent krav på samme reskontro,
   KID uendret) i samme tx som skrittet; migration 0017
   `invoice_reminder` insert-only med **lagret rendret dokument**
   (gjenutstedbart for alltid). "Forfalt" alltid beregnet, buckets
   1-14/15-30/30+. `…/invoices/overdue` + `…/invoices/{iid}/reminders`
   (POST `?preview=true` = forhåndsvisning); portal: Forfalt-stat,
   forfalte-kort m/ purreskjema (foreslått neste steg, forhåndsvis →
   registrer), historikk m/ tekstnedlasting. Browser-verified
   (tests/seed_browser.rs seeds a demo w/ static JWKS for that).
26. ✅ Dimensjoner (docs/dimensjoner.md, closed #37): avdeling/prosjekt
   as first-class, hash-covered line data — **hash format v3** (marker
   + per-line avdeling/prosjekt codes; v1/v2 verify forever, golden
   tests pin all three digests). Migration 0018: `dimension` registry
   (insert + rename + open/close ONLY — the code is permanent because
   it is hashed; trigger + column grants enforce), nullable
   avdeling_id/prosjekt_id on entry (+ codes on invoice_line so
   kreditnota mirrors). Posting resolves and validates dims up front;
   avsluttet rejects new postings like a locked period. Resultat takes
   `avdeling=`/`prosjekt=` filters (balanse deliberately not);
   kontospesifikasjon carries the codes; SAF-T exports
   AnalysisTypeTable ("AVD"/"PRO") + per-line Analysis with amounts,
   XSD-validated. `GET/POST …/dimensions`, `PUT …/dimensions/{kind}/{code}`;
   dims accepted on innboks-bokfør and faktura lines. Portal: registry
   card (Bilag), pickers in bokfør + faktura forms, resultat filter.
27. ✅ Faktura-PDF (docs/faktura.md, first half of #32):
   `regnmed-core::pdf` — hand-rolled deterministic PDF writer (standard
   fonts, WinAnsi/CP1252, width-based right alignment, ~3 KB per
   document, no engine; xref structure test-verified) + `fakturapdf`
   layouts per bokføringsforskriften §5-1-1 (mva spesifisert per sats,
   "MVA"/"Foretaksregisteret", KID/kontonummer; kreditnota variant;
   pagination). **Stored as a voucher attachment in the issuing
   transaction** — serving is a hash-checked DB read, nothing renders
   on the request path. Purringer render stored text → PDF on demand
   (`?format=pdf`, Courier). Migration 0019 kontaktinfo (company
   address/bank_account/orgform, party address/email — editable, never
   hashed) + `GET/PUT …/settings` (PUT admin-only),
   `PUT …/parties/{pid}/contact`, `GET …/invoices/{iid}/pdf`. Portal:
   Firmaopplysninger card (Oversikt), Kontaktinfo (party page), PDF
   buttons (Faktura, purrehistorikk). PDF visually verified.
28. ✅ E-postutsendelse (docs/faktura.md, closed #32): ONE rail for all
   outbound mail — regnid's `OutboundMail` gained serde-defaulted
   `reply_to` + base64 `attachments` (wire format pinned by test in
   regnid; SMTP multipart + Brevo attachment support in its
   transports), and regnmed publishes to the same JetStream stream
   (`REGNID_MAIL`/`regnid.mail.send`, mirrored constants in
   `regnmed-api::mailq` — a documented wire contract, regnid is never
   vendored). Sending is explicit human action:
   `POST …/invoices/{iid}/send`, `POST …/reminders/{rid}/send`
   (recipient = party e-mail, overridable; reply-to = company e-mail);
   migration 0020 `utsendelse` insert-only log whose id doubles as
   Nats-Msg-Id (log row ≡ queue message, dedup for free) + company
   email column. AppState carries Option<jetstream::Context> — no
   NATS_URL → clear error, not pretence. NATS_URL added to
   regnmed-api's base deployment (NATS already in the cluster).
   Integration test spawns a real nats-server (skips without) and
   base64-decodes the stored PDF back off the stream.
29. ✅ Repeterende faktura (docs/faktura.md, closed #30): editable
   templates (party + lines + intervall + neste/slutt, migration
   0021), generation issues ORDINARY invoices via the refactored
   `create_invoice_in` — template lock + invoice + insert-only run row
   + neste_dato advance in ONE tx, partial unique index makes a period
   ungenerable twice; failures log + retry, behind templates catch
   up; `{måned}`/`{år}` periodetekst (pure helpers in core, month
   clamping documented). Daily CronJob `regnmed generate-invoices`
   (deploy/base, anchor pattern); `merk_utsendelse` only MARKS in the
   run log — sending stays human. `…/invoice-templates` CRUD +
   `/generate` + `/runs`; `from_invoice_id` = "gjenta denne". Portal:
   Repeterende card (generer nå, stopp/start), Gjenta on invoice rows.
30. ✅ Tilbud→ordre→faktura (docs/faktura.md, closed #31 — M3
   betalingsoppfølging track complete): salgsdokument outside the
   ledger (migration 0022) — tilbud freely editable until
   akseptert/avslått, ordre frozen; own gap-free series per kind
   (rejected tilbud = history, not a hole); one-way statuses; at most
   one ordre per tilbud (unique index); ordre→faktura runs
   create_invoice_in with status flip + invoice link in ONE tx (one
   ordre → one faktura); chain tilbud→ordre→invoice in listings.
   `fakturapdf` gained a Dokumenttype enum — TILBUD/ORDREBEKREFTELSE
   render on demand, no KID/betalingsinfo. `/quotes` + `/orders`
   endpoints; portal Tilbud og ordre card (statusknapper, → Ordre,
   → Faktura, PDF).
31. ✅ Timeføring (docs/timer.md, closed #38): time_entry with INTEGER
   minutes (1..=1440), prosjekt from the dimension registry (active
   required), fakturerbar + timesats_ore (migration 0023). Editable
   until (a) månedslås — insert-only timesheet_lock exactly like
   period_lock — or (b) fakturert (one-way invoice link); BOTH
   enforced by a DB trigger whose single exception is the pure
   billing-marker update (lock for lønn, then bill).
   Fakturagrunnlag: unbilled billable hours grouped per (prosjekt,
   sats) → milli-hour invoice lines carrying the prosjekt DIMENSION,
   issued via create_invoice_in with entries marked fakturert in the
   same tx. /companies/{id}/timesheet endpoints (own entries; admin
   corrects all; lock admin-only); portal Timer section (min uke,
   per-prosjekt, ufakturert → Lag faktura).
32. ✅ Produktregister + enkelt varelager (docs/produkter.md, closed
   #39): register editable EXCEPT nummer (permanent, trigger) and
   never deletable — document lines COPY the values at issue
   (`resolve_product_line`; one shared DocLineRequest for
   faktura/tilbud/maler, `TemplateLineDraft` unified into
   `InvoiceLineDraft` w/ product_id) so register edits never touch
   issued documents. Migration 0024: insert-only `inventory_movement`
   (kjop m/ kostpris per enhet; salg AUTO-inserted inside
   create_invoice_in — kreditnota returns stock; justering krever
   notat). Beholdning = SUM(antall_milli); verdi =
   gjennomsnittsmetoden as a pure fold in `regnmed-core::lager`
   (milli-units × øre, half-away rounding; negativ beholdning synlig,
   aldri skjult). Varetelling i én tx: justeringer + verdibilag mot
   bokført lagersaldo (1460/4390 defaults). `/products` +
   `/inventory` endpoints; portal Produkter section + produkt-pickers
   på faktura/tilbud.
33. ✅ Anleggsregister og avskrivninger (docs/anlegg.md, closed #40):
   migration 0025 `asset` — INSERT + the one-way avhending transition
   only (trigger + column grants; bokført verdi ALDRI stored:
   kostpris − SUM(logged avskrivninger)). Lineære avskrivninger as
   ordinary vouchers via the repeterende-faktura pattern: one tx per
   asset-month (voucher dated month-end + run row), partial unique
   index forbids double depreciation, failures logged w/ detail;
   `regnmed depreciate` CLI + monthly CronJob (also fixed the missed
   prod DATABASE_URL patch on the generate-invoices CronJob).
   Skattemessig: saldogruppesatser a–j seeded in satsregisteret
   (sktl. §14-43, kadens-exempt); `saldo_rapport` computes per gruppe
   from scratch (grunnlag/avskrivning/utgående + midlertidige
   forskjeller vs bokført), fails loudly outside rate coverage;
   negativ saldo/§14-45/§14-47 deliberately manual (documented).
   Avhending: gevinst (3880) / tap (7880) + one-way close in one tx;
   aktiveringsgrense warning (never refusal) at registration. Core
   `regnmed-core::anlegg` (manedsbelop sums EXACTLY, saldo_ar).
   `/companies/{id}/assets…` endpoints; portal Anlegg section.
34. ✅ Utlegg og kjøregodtgjørelse (docs/utlegg.md, closed #42):
   migration 0026 `expense` — the innboks discipline on refusjonskrav:
   content immutable from submission (receipt bytea + SHA-256, trigger
   + column grants), one-way innsendt→godkjent/avvist→utbetalt
   (avvisning krever begrunnelse; transitions enforced by trigger).
   Godkjenning in ONE tx: kostnad + inngående mva (split_gross,
   dato-riktig sats) mot mellomregning (2910 default), **kvitteringen
   kopiert inn som vedlegg på bilaget** (oppbevaringsplikt).
   Kjøregodtgjørelse: km × statens sats PÅ KJØREDATOEN fra
   satsregisteret, satser LAGRET på kravet ved innsending (evidence);
   trekkfri/trekkpliktig split i `regnmed-core::utlegg`; trekkpliktig
   del = tydelig varsel (a-melding #46 ikke bygget) — aldri skjult.
   Utbetaling: mellomregning→bank (1920) one-way; remittering (#33)
   overtar steget senere. Selvgodkjenning tillatt i v1 (#47
   attestering legger seg oppå). `/companies/{id}/expenses…`
   endpoints; portal Utlegg section.
35. ✅ Flervaluta (docs/valuta.md, closed #44): **hash format v4** —
   per-entry valutainformasjon (ISO code, valutabeløp i cent, kurs i
   mikro-NOK) as ONE `Option<Valuta>` field on EntryDraft, covered by
   the canonical serialization ("v4" marker; v1–v3 verify forever,
   golden tests pin all four digests). NOK is authoritative; posting
   sanity-checks cent × kurs within 1 kr (catches unit mistakes).
   Migration 0027: global dated `valutakurs` (append-only, kilde per
   row; lookup = newest notering ≤ dato), entry columns,
   reskontro_match.valuta_cent, invoice.valuta.
   `regnmed-gov::norgesbank`: SDMX-JSON client for Norges Banks åpne
   API (UNIT_MULT per-100 quotes handled; decimal-string parsing, no
   floats; vendored sample in docs/valuta/); `regnmed fetch-rates`
   CLI + portal fetch button (live-verified). Faktura i valuta:
   line amounts in cent, per-line NOK conversion at dagskurs
   (receivable = exact sum of parts), PDF motverdi note, kreditnota
   reverses at ORIGINAL kurs. `match_valuta`: agio (8060/8160) +
   party-carrying transfer entry posted in the SAME tx as the match
   rows — both NOK remainders reach exactly zero (proportional to
   each entry's own booked relation, no external kurs). Urealisert
   kursregulering: voucher + reversal (reverses-linked) in one tx,
   NOT idempotent (documented). SAF-T CurrencyCode/CurrencyAmount/
   ExchangeRate, XSD-validated. Valutakontoer i bank + sikring
   deliberately out (NOK bank first).
36. ✅ Mva-terminordninger (docs/mva.md, closed #51 — M2 tail):
   `Terminordning` in core (to-maneder/arlig/primaernaering) owns
   ordning-aware perioder + LEVERINGSFRISTER (sktfvf. §8-3 incl.
   særregelen 31. aug for 3. termin; årstermin 10. mars; primærnæring
   10. april). Migration 0028 `mva_terminordning`: dated per company,
   append-only, note = vedtaksreferanse — the ordning Skatteetaten
   GRANTED is recorded, never inferred. Spesifikasjon, mva-melding
   (skattleggingsperiodeAar for yearly — XSD's own distinction, both
   yearly ordninger render identically) and portal picker follow the
   ordning; periode numbers outside it refused everywhere (API + CLI).
   `GET/POST /companies/{id}/mva/terminordning` (POST admin).
37. ✅ Betalingsliste og remittering (docs/betaling.md, closed #33):
   `regnmed-core::pain001` — hand-rolled deterministic
   pain.001.001.03 (official XSD vendored in docs/pain001/,
   validated in tests/CI; KID as SCOR structured reference,
   EndToEndId = run-item id, integer-øre CtrlSum) + norsk
   kontonummer MOD11 (same cyclic weights as KID mod11;
   normalisering av punktum/mellomrom). Migration 0029:
   party.bank_account (validated before storage via
   update_party_contact); `payment_run`/`payment_run_item` — one-way
   utkast→godkjent→utbetalt (+utkast→annullert), trigger-enforced;
   items SNAPSHOT creditor data; create and approve are SEPARATE
   audited actions (four-eyes friendly, enforcement = #47); approval
   renders + stores the file w/ SHA-256 (download hash-checked);
   settle posts ONE utbetalingsbilag + reskontro match per item in
   one tx — bank import then matches that voucher via the ordinary
   engine. v1 = domestic NOK/BBAN; IBAN/BIC + filutveksling/PSD2
   later. `/companies/{id}/payments…` endpoints; portal
   Betalingsliste card under Bank + kontonummer on party page.
38. ✅ Nøkkeltall og likviditet (docs/rapporter.md, closed #36): one
   endpoint `GET …/reports/nokkeltall?year=` — pure SUM queries only:
   resultat hittil i år vs SAMME DATO i fjor (3xxx–8xxx,
   presentasjonsfortegn), månedskolonner, likviditetsbilde (19xx +
   kundereskontro − leverandørreskontro − beregnet mva-netto for
   inneværende periode = disponibelt), and the next two mva-frister
   from the company's Terminordning. Year steers hittil/månedene;
   likviditet + frister are always NOW. Portal: Nøkkeltall card on
   Oversikt with CSS-only month bars (no chart library — frugality).
   Prognoser/budsjettavvik deliberately left to #41.
39. ✅ Attestering (docs/attestering.md, closed #47): intern kontroll
   som flyt, OPT-IN — uten policy er alt som før. Migration 0030:
   `attestation_policy` append-only (nyeste rad gjelder; aktiv,
   beløpsgrense, utpekt attestant), `attestation` insert-only
   beslutningsspor (nyeste beslutning gjelder, avvisning krever notat
   — DB-sjekk), `payment_run.created_by_person` (identitet, ikke
   visningsnavn; 0029-vakten utvidet). Håndhevingen ligger INNE i
   transaksjonene, aldri i portalen: `bokfor_inbox_document` krever
   godkjent attestering når debetsummen ≥ grensen og nekter samme
   person å både attestere og bokføre; `approve_run` krever annen
   godkjenner enn oppretter (fire øyne på penger ut); `approve_expense`
   nekter selvgodkjenning (#42 v1-oppførselen bevart uten policy).
   `/companies/{id}/attestering/policy` (POST admin), `…/members`,
   `…/inbox/{doc}/attester`, `…/inbox/{doc}/attestering` (revisor
   leser sporet); innboks-listingen bærer attesteringsstatus. Portal:
   Til attestering-kort øverst i Bilag (kø + policyskjema) + kolonne i
   innboksen. Flertrinns kjeder/beløpsmatriser bevisst utenfor v1;
   `target_kind` gjør utvidelse til en migrasjon.
40. ✅ Budsjett og avviksrapport (docs/budsjett.md, closed #41): the
   only number in the system that is an OPINION, treated as such.
   Migration 0031: `budget` (per company/år, versjonert; utkast fritt
   redigerbart, fastsett = enveis frys av rad OG linjer via trigger;
   utkast kan forkastes, fastsatte aldri) + `budget_line` (konto ×
   måned, unik). En revisjon er en NY VERSJON — derfor kan
   avviksrapporten alltid navngi planen den måler mot (nyeste
   fastsatte som standard, ellers nyeste utkast, status alltid i
   svaret). Linjer lagres i PRESENTASJONSFORTEGN (inntekt positiv,
   kostnad positiv — budsjettet skrives slik det leses); faktiske tall
   konverteres med `regnskap::presentasjon_ore` (regelen ett sted) før
   sammenligning. Bare resultatkontoer (klasse 3–8);
   likviditetsbudsjett bevisst utenfor. `regnmed-core::budsjett`:
   ren avvik-fold (NS 4102-seksjoner, hittil t.o.m. valgt måned,
   konto som bare finnes på én side blir MED) + `juster_ore` for «fra
   fjoråret ±X %» (basispunkter, half-away, ingen flyttall).
   `/companies/{id}/budgets…` + `GET …/reports/avvik`; portal:
   Budsjett-fane under Rapporter m/ redigerbart konto×måned-rutenett.
41. ✅ Migrering: kontakter og åpne poster, filtier (docs/migration.md,
   closed #19): SAF-T flytter hovedboken, dette flytter resten. CSV
   fra ethvert norsk system, layout lest av OVERSKRIFTENE (ingen
   profil per leverandør) — bankcsv-mønsteret, med de delte
   primitivene løftet ut i `regnmed-core::csvutil` (bankcsv bruker nå
   samme kode; testene beviste at oppførselen står). Nytt:
   `regnmed-core::migreringcsv` (kontakter + åpne poster;
   `find_column_ranked` = prioritert kolonnevalg, så **restbeløp
   vinner over fakturabeløp** — bruttoimport ville blåst opp
   reskontroen i stillhet). Retningen bestemmes av parts-typen (kunde
   debet / leverandør kredit), aldri av filens fortegn; kreditnota
   beholder sitt eget. `regnmed-db::migrering`: kontakter idempotent
   (orgnr → numerisk nummer → navn; kundenr 10001 forblir
   partsnummer), åpne poster som ETT bilag med partslinjer mot
   motkonto (2050) i én tx — krever NULL saldo på reskontrokontoen og
   sier fra med tallet ellers (postene ERSTATTER samlelinjen), og
   setter reskontro-flagget tilbake som åpningsbalansen utsatte.
   `POST …/import/contacts` + `…/import/open-items?preview=true`
   (admin); portal: «Importer fra et annet system» i Reskontro med
   forhåndsvisning før bokføring. API-tieren per leverandør krever
   nøkler og er dokumentert som neste steg, ikke lovet.
42. ✅ EHF/PEPPOL — dokumenttieren (docs/ehf.md, closed #14): de to
   endene vi kan stå inne for uten aksesspunkt. UT:
   `regnmed-core::ehf` rendrer PEPPOL BIS Billing 3.0 (UBL 2.1)
   hand-rolled og deterministisk fra fakturaens LÅSTE rader —
   `GET …/invoices/{iid}/ehf`, ikke lagret som vedlegg (PDF-en ER
   salgsdokumentet; EHF-en er en transportkonvolutt av samme tall).
   ICD 0192 som deltakerid, mva-sats fra fakturadatoen (samme daterte
   oppslag som posteringen), ett TaxSubtotal per sats, linje uten
   mva-kode blir Z (ikke utelatt), kreditnota m/ 381 +
   BillingReference. Offisiell UBL 2.1 XSD vendored i docs/ehf/ og
   kjørt med xmllint i tester OG CI. INN: `regnmed-core::ehf_import`
   (tolerant, camt053-stil) leser mottatt EHF i innboksen til et
   BOKFØRINGSFORSLAG — `GET …/inbox/{doc}/ehf`, utledet av originalen
   hver gang, aldri lagret (bedre forslag gjelder også gamle
   dokumenter); leverandør matchet på orgnr, advarsler følger med i
   stedet for å stoppe. Portal: EHF-knapp på fakturalinjer + EHF-knapp
   på XML i innboksen som fyller bokføringsskjemaet.
   ÆRLIG BEGRENSNING dokumentert: XSD ≠ PEPPOL Schematron
   (forretningsreglene kjøres av aksesspunktet); E/AE-kategorier
   utledes ikke. Transporten (SMP-oppslag + AS4) er egen tier som
   krever leverandøravtale.
43. ✅ Bilagstolkning (docs/bilagstolkning.md, closed #34): forslag,
   aldri bokføring — det finnes INGEN automatisk bokføringsvei, heller
   ikke «over beløpsgrense X». `GET …/inbox/{doc}/forslag` svarer for
   ethvert dokument og sier `kilde`: ehf (eksakt) → pdf-tekst /
   tekst (heuristikk) → ingen (skann uten tekstlag foreslår
   INGENTING og sier fra). Nytt: `regnmed-core::pdftekst` (PDF-ens
   egne innholdsstrømmer, rå + Flate via ny dep flate2/rust_backend,
   Tj/TJ-strenger, WinAnsi; returnerer None på mojibake — testet med
   søppel-PDF) og `regnmed-core::bilagstolk` (heuristikk avgjort av
   KONTROLLSIFRENE vi allerede validerer: orgnr-MOD11, KID-MOD10/11,
   kontonummer-MOD11; «å betale» slår «sum»; en linje MED tall holder
   seg til sin egen linje — ellers leses totalen under «mva» som
   mva). Hvert felt bærer sin begrunnelse, vist i UI-et.
   Kontoforslaget = kontoen samme leverandør sist ble bokført på (ren
   spørring over egen historikk). Portal: Forslag-knapp på alle
   innboksdokumenter + tydelig «Forslag»-banner m/ kilde og
   begrunnelser over skjemaet. OCR = valgfri sidecar senere, samme
   endepunkt.
44. ✅ E-post-inn (docs/epost-inn.md, closed #35 — **M5-sporet tomt**):
   plattformen har ÉN mail-rail — utgående `regnid.mail.send`,
   innkommende `regnid.mail.received` (wire-kontrakt speilet i
   `regnmed-api::mailq_in`; MX-en bor i regnid, aldri vendored). Uten
   NATS_URL finnes ingen konsument og e-post-inn er av; portalen sier
   det. Migration 0032: `company_mail_inbox` (adressen er en
   KAPABILITET — `bilag-<navn>-<tilfeldig>`, lesbar men ikke gjettbar,
   roterbar, og en tilbakekalt adresse kan ikke gjenoppstå),
   `mail_sender_allow` (full adresse eller helt domene — ingen
   jokertegn), `inbox_mail` insert-only logg m/ brødtekst + rå melding,
   `inbox_mail_attachment` (dekodet én gang, SHA-256 — derfor kan
   karantene slippes gjennom uten at avsender sender på nytt),
   `inbox_document.inbox_mail_id` (0015-vakten utvidet: grants OG
   trigger). UKJENT AVSENDER → KARANTENE, aldri stille import (hvem som
   helst kunne fylt innboksen) og aldri stille forkasting (et bilag
   noen sendte forsvinner). Dedup på Message-Id; `uploaded_by` =
   avsenderadressen. `/companies/{id}/inbox/settings…` + `…/inbox/mail`
   + release/reject (admin); portal: E-post-inn-kort i Bilag.
   Integrasjonstesten kjører mot ekte nats-server.
45. ✅ Maskin-tilgang til API-et (docs/integrations.md + docs/api.md,
   closed #45): «tokenet beviser identitet, regnmed avgjør hva den får
   gjøre» — samme setning som for mennesker, og derfor INGEN ny
   autorisasjonsvei. En integrasjon er en `person` med
   kind='integrasjon' (migration 0033), så tilgangsoppslag,
   attribusjon og revisjonsspor er identiske for robot og menneske.
   Identiteten kommer som client_credentials fra IdP-en; regnmed
   utsteder aldri egne API-nøkler. **client_credentials finnes nå i
   regnid** (dens migrasjon 0007): grantet er av som standard per klient
   og gis uttrykkelig med `add-client --grant-type client_credentials
   --confidential`, slik at en vanlig konfidensiell webklient ikke
   stilltiende blir en maskinaktør. Verifisert på tvers 2026-07-27 —
   ekte regnid-token → regnmed `/me` svarer 200 med sub = client_id og
   companies: []. Gjenstår bare utrulling av regnid med 0007.
   regnmed-siden virker med et hvilket som helst token fra issueren der
   sub = client_id. `integration_grant` er
   modellert som oppdrag (valid_to EKSKLUSIV → tilbakekalling virker
   straks); admin er bevisst ikke grantbart til en maskin; en
   klient-id som tilhører et menneske MED tilgang kan ikke kapres
   (tomt person-skall kan konverteres). created_by = integrasjonens
   navn, satt ved registrering — tokenet kan ikke døpe om roboten.
   Rate limit: token-bucket per prosess i AuthPerson-ekstraktoren (ett
   seam, ingen endepunkter kan glemme det), 429 m/ tydelig melding;
   per replika, dokumentert som bevisst avveining. Logg: endrende kall
   i sin helhet + dagsteller for alle kall.
   `/companies/{id}/integrations…`; portal: Integrasjoner-kort under
   Oppdrag. NYTT: docs/api.md er den offentlige endepunktreferansen
   (generert fra rutetabellen).
46. ✅ Mobil-PWA (docs/portal.md, closed #48): samme portal, ingen
   app-butikk. `/manifest.webmanifest` + `/sw.js` + genererte ikoner
   (scripts/build-icons.py — hand-rolled PNG m/ zlib, sjekkes inn som
   app.css) servert fra binæren. REGELEN i service workeren:
   **hovedboken caches ALDRI** — bare app-skallet, nett-først, og
   endrende forespørsler går aldri gjennom cachen (testet ved at sw.js
   ikke nevner /companies/). Kvitteringsfoto = `capture="environment"`
   rett til det uendrede innboks-endepunktet. Offline-kø BARE for
   opplastinger (IndexedDB): bildet hashes i telefonen, sendes med
   `?sha256=`, og serveren avviser (a) innhold den allerede har — så
   en kø-retry ikke gjør ett bilag til to — og (b) hash som ikke
   stemmer med bytene (skadet underveis). Køen dropper et bilde
   serveren avviste, beholder det når nettet svikter. Responsivt:
   menyen vannrett under sm, kortkropper ruller egne tabeller;
   temakontrakten urørt. Verifisert i 375×812-viewport (og på nytt
   etter Svelte-flippen 2026-07-29; sw.js lister nå adresser i stedet
   for filnavn, siden Vite hasher dem).
47. 📌 Avvikende regnskapsår (#52) — BEVISST IKKE BYGGET. Saken sier
   det selv: målgruppen er kalenderår, dette er et sporet
   omfangsvalg, og leveransen er at antakelsen skal være eksplisitt.
   Gjort: `regnmed-core::regnskapsar` navngir antakelsen
   (`regnskapsar(dato)` = kalenderår i dag, `regnskapsar_periode`) med
   enhetstest som fester den, og posteringens `fiscal_year`
   (bilagsnummerserien) + SAF-T-ens `year=` går gjennom sømmen — et
   spredt `.year()` er en antakelse ingen finner igjen.
   docs/regelverk.md har hjemmelen (rskl. §1-7) og et VERIFISERT
   kostnadskart m/ fil og linje for det som gjenstår (budsjett- og
   nokkeltall-SQL bruker `extract(year …)`, asset saldo_rapport,
   årvelgerne i portalen) + kravet om en datert regnskapsårsdefinisjon
   per selskap (mva_terminordning-mønsteret). MÅ IKKE følge etter:
   mva-terminer (mval. §15-1) og skattemessig saldo (sktl. §14-40 flg.)
   er kalender-/inntektsårsforankret uansett.
48. ✅ Aksjeeierbok og aksjonærregisteroppgaven (docs/aksjonaer.md,
   closed #43): to ting som ofte blandes, holdt fra hverandre.
   AKSJEEIERBOKEN er lovpålagt i seg selv (aksjeloven §4-5) og
   modellert som hovedboken: migration 0034 gir `shareholder`
   (identiteten PERMANENT — trigger + kolonnerettigheter; kontaktinfo
   redigerbar; ingen slettes), `share_event` (insert-only; `antall`
   alltid POSITIVT, retningen ligger i typen, så en rad kan ikke motsi
   seg selv; en overdragelse = to rader i én tx m/ motpart begge veier)
   og `dividend` (ETT vedtak, ikke ett beløp per eier — den enkeltes
   utbytte er beholdning på beslutningsdatoen × per aksje, så delene
   kan ikke avvike fra helheten; bokføres 2050→2800 i samme tx).
   Eierandelen LAGRES ALDRI. PERSONVERN som designvalg: §4-5 krever
   FØDSELSDATO, RF-1086 krever FØDSELSNUMMER — `Aksjonaer` (portal +
   API) bærer bare datoen, utledet av nye `regnmed-core::fnr` (MOD11 ×2,
   D-nummer dag+40, H-nummer mnd+40 og **syntetisk mnd+80** som er
   Skatteetatens Tenor-konvensjon og står i deres eget RF-1086-eksempel;
   århundret fra individnummeret). Nummeret leses ÉTT sted — når
   oppgaven bygges — og integrasjonstesten fester begge sider.
   OPPGAVEN: `regnmed-core::aksjonaeroppgave` rendrer hovedskjema
   (RF-1086) + ett underskjema per aksjonær (RF-1086-U) hand-rolled i
   Altinns Skjema-dialekt (gruppeid/orid, XSD-ens sekvens), validert mot
   Skatteetatens offisielle XSD-er (vendored docs/aksjonaer/) i
   enhetstester, integrasjonstest OG CI. HASTER: **fra juni 2026 er
   sluttbrukersystem eneste leveringsvei** — Altinn.no og papir er
   avviklet. ÆRLIG BEGRENSNING: transaksjonstypekodene er IKKE publisert
   (begge felt er ubundet Tekst35, rettledningen navngir uten koder,
   listen går til SBS-kanalen). Vi leverer bare `N` (stiftelse/
   nyemisjon, fra etatens eget eksempel) og NEKTER HØYLYTT for resten —
   en feil transaksjonstype flyter inn i aksjonærens RF-1088 og endrer
   inngangsverdi/skjermingsgrunnlag. Forhåndsvisningen dør ikke av det:
   den viser tallene + `leverbar:false` + hindringene. Innsending
   venter på scope `skatteetaten:innrapporteringaksjonaerregisteroppgave`
   OG Altinn systembruker (docs/gov.md). Portal: Aksjonærer-seksjon.
49. 🔨 Lønn, FØRSTE DEL (docs/lonn.md, #46 — resten står åpen):
   fastlønn, prosenttrekk fra skattekortet, arbeidsgiveravgift per sone
   og feriepengeavsetning, bokført som ETT bilag i én transaksjon
   (5000/2600/2930 + 5090/2940 + 5400/2770; netto til 2930, ikke bank —
   utbetaling er betalingslistens jobb). Migration 0035: `employee`
   (identitet permanent), `payroll_run`/`payroll_line` innsettings-bare,
   én kjøring per måned. Satsene er DATA i satsregisteret m/ kilde
   (aga I 14,1 → V 0 %, ferieloven §10 10,2/12,5 %); den ekstra
   aga-en over 750 000 finnes ikke fordi den ble fjernet i 2025.
   Utbetalte feriepenger trekker ned GJELDEN, de er ingen ny kostnad
   (egen test — ellers kostnadsføres de to ganger). Trekkreglene:
   feriepenger trekkfrie, halv skatt i desember — begge er tidfesting,
   ikke gaver, fordi skattekortprosenten er beregnet over 10,5 måneder.
   NEKTER HØYLYTT: tabelltrekk (trekktabellene er Skatteetatens
   datafiler, en tilnærming blir den ansattes restskatt) og sone Ia
   (fribeløpet på 850 000 er bagatellmessig støtte som også forbrukes
   utenfor regnmed — avvises FØR satsoppslaget, ellers ville
   feilmeldingen bedt om å legge inn satsen, som er selve feilen).
   Ansattlisten viser fødselsdato, ikke fødselsnummer.
   Siden bygget: portal-seksjon, lønnsslipp som PDF, timelønn fra
   låste timer, og **aga-avsetning på ikke-utbetalte feriepenger**
   (5405/2780) — modellert som et MÅL, ikke en strøm av tillegg: etter
   hver kjøring er påløpt aga satsen av det som faktisk skyldes, og
   kjøringen bokfører differansen, så utbetaling, satsendring og gjeld
   uten avsetning retter seg selv. Skyldig og allerede avsatt utledes
   per ansatt av de innsettings-bare lønnslinjene; negativ gjeld gir
   null avsetning, aldri negativ avgift; feriepengegjeld
   lønnshistorikken ikke forklarer (åpningsbalanse, import) gir en
   ADVARSEL med beløpet framfor en oppdiktet fordeling. Fant og fikset
   underveis: `lonnskostnad_ore` telte utbetalte feriepenger som
   kostnad og utelot avgiftene, og en måned med bare ferieavvikling
   kunne ikke kjøres i det hele tatt (nullinjer avvises av
   bilagsvalideringen — linjene utelates nå når de er null).
   **IKKE bygget, i prioritert rekkefølge: a-meldingen (selskapet må
   fortsatt levere den selv), skattekort-API, tabelltrekk.**
50. 📌 Skattemelding og næringsspesifikasjon (#11) — KARTLAGT, IKKE
   BYGGET (docs/skattemelding.md). To funn endrer saken. (a) Innsending
   går ikke på maskinporten-skinnen: Skatteetaten sier selv
   «Validering og innsending må fortsatt gjøres med ID-porten», og
   Maskinporten gir bare lesetilgang for inntektsår 2025 — altså er
   **#11 nedstrøms #26** (ID-porten-føderering), noe ROADMAP-en ikke
   fanget. (b) XSD-er, kodelister og eksempler ligger åpent i
   Skatteetaten/skattemeldingen (aktivt vedlikeholdt), og strukturen
   passer uvanlig godt — `resultatregnskap` og `balanseregnskap` er
   VALGFRIE elementer vi kan fylle helt ut fra
   `regnmed-core::regnskap` i dag, og grupperingskodelisten er den
   samme vi alt bruker i SAF-T. MEN: versjon-per-inntektsår er
   verifisert bare til og med 2024 (2021→v2, 2022→v3, 2024→v5 fra
   etatens egne eksempelfiler). v6/v7 finnes, regelen
   `versjon = år − 2019` treffer alle punktene, men det er en
   slutning og ikke en kilde — å skrive årgangsregisteret på den ville
   brutt #50. Ett spørsmål til brukerstøtte lukker det.
51. ✅ Tilgangssporet (docs/auth.md, closed #56 #59 #53 #54 #55 #60 #61
   #58): ÉN vakt i stedet for 22 kopier (`regnmed-api::tilgang::krev` —
   eneste sted `company_roles` slås opp; ingen tilgang = 404, aldri
   403); 72-rettighets vokabular som Rust-enum m/ norske slugger
   (`FAKTURA_SKRIV`, `LONN_KJOR`), `_EGNE`/`_ALLE`-par der `_ALLE`
   medfører `_EGNE`; roller = SETT av rettigheter, ikke stige —
   `ansatt` (selvbetjening: egne timer/utlegg/lønnsslipp, ikke
   hovedbok), `les` (IKKE lønn), `revisor` (les + lønn, følger
   revisjonsoppdrag), `bokforing`, `admin`; rettigheter UNIONERES over
   alle veier inn; ukjent rollenavn gir INGENTING (fail-closed).
   Medlemsadministrasjon (migration 0037): invitasjon til
   E-POSTADRESSE (person finnes ikke før første innlogging), løses inn
   i `/me`, svaret røper aldri om adressen er bruker; siste admin kan
   ikke fjerne seg selv (`for update` inne i tx); insert-only
   `company_member_change`. Egendefinerte roller (migration 0039):
   selskapet komponerer av vokabularet, innebygde navn reservert,
   tilgangsstyrende rettigheter (`MEDLEM_ADMIN` m.fl.) kan IKKE
   delegeres, roller deaktiveres — slettes aldri. Portal: Roller-kort
   m/ beskrivelser servert fra koden (`Rett::beskrivelse()`/`gruppe()`
   — ingen andre kopi). Matrisen i docs/auth.md er MASKINSJEKKET
   (`tests/grupper/matrise.rs` genererer og krever likhet). Lærdom, festet i
   docs: en autorisasjonstest som ikke er sett feile er ikke
   verifisert (axum kjører `Json`/`Query`-uttrekk FØR vakten — ugyldig
   kropp gir 422/400 og måler ingenting).
   ETTERSLEP (closed #62): `regnmed-db::roller` skrev flerstegs rett mot
   poolen — nå ÉN transaksjon per endring, med `for update` på rollen
   før rettighetslisten skrives om. Uten det kunne (a) en rolle bli til
   uten rettigheter og uten rad i `company_role_change`, og (b)
   tilgangsvakten lese MELLOM `delete` og `insert` og se en TOM liste,
   så den som har rollen mistet tilgangen et øyeblikk i en helt annen
   forespørsel. Unik-bruddet gjenkjennes nå på SQLSTATE 23505, ikke på
   constraint-navnet. Begge testene er sett feile uten rettingen.
   Dokumentert samtidig (docs/auth.md §7): en invitasjon til en
   egendefinert rolle som deaktiveres FØR innløsning gir medlemskap
   uten tilgang — valgt fail-closed, ikke oppdaget.
52. ✅ Ingen plattformadministrator (docs/auth.md §8, closed #57):
   avgjørelsen tatt UTTRYKKELIG — ingen tilgangsvei krysser
   selskapsgrenser, ingen leverandørbakvei; festet i test
   (`an_admin_crosses_no_company_boundary`: admin i A er fremmed i B,
   404 på alt, `/me` tier). Støttevei = kunden inviterer selv
   (minste rolle) eller gir oppdrag — synlig, logget, trekkbart samme
   dag. Selskapet uten admin: dokumentert NØDPROSEDYRE via databasen
   m/ skriftlig samtykke; migration 0040 gir sporet et eget navn
   (`kilde='nodprosedyre'`) og check-constraint som NEKTER innslaget
   uten samtykkereferanse i `notat` — loggen kan ikke lyve om akkurat
   det innslaget den finnes for å fange. Driftsoppgavene
   (anchor/generate-invoices/depreciate/…) går bevisst utenom vakten —
   maskinelle, ingen menneskeavgjørelser. Bygges en plattformrolle
   likevel en gang, står kravlisten i #57 (logget, varslet,
   tidsbegrenset).
53. ✅ Abonnement (docs/abonnement.md, closed #65): regnmeds egen kasse.
   PRINSIPP håndhevet i tilgangsvakten: sperret abonnement stopper
   ENDRINGER (`Rett::endrer()` — uttømmende match, ny rettighet må
   velge side), ALDRI lesing, eksport eller styring av selskapet
   (tilgang/oppdrag/integrasjoner åpne så noen kan rydde opp) —
   hovedboken tas aldri som gissel. Status BEREGNES aldri lagres
   (`regnmed-core::abonnement`): prove (30 d fra opprettelse) → aktiv
   (dekkende rad) → frist (14 d) → sperret; frist regnes fra seneste av
   prøvetid/oppsigelse. Migration 0041: `abonnement` daterte
   dekningsrader m/ note (tegn=insert, avslutt=update valid_to alene),
   `abonnement_pris` datert insert-only prisliste m/ kilde
   (satsregister-mønsteret; PRISVEDTAK 2026-07-28 i migration 0042:
   basis 49 kr/mnd selvbetjent, standard 99 kr/mnd m/ e-postsupport —
   alt inkludert i begge, brukere koster aldri, skillet er
   supportkanalen og ALDRI en funksjonssperre; konkurrentene verifisert
   199–425 + moduler), `abonnement_faktura_run` unik per
   (selskap,år,måned) i SAMME tx som fakturaen; eksisterende selskaper
   sådd med åpen dekning. Fakturering = DOGFOOD: egen motor i
   driftsselskapets hovedbok (KID, reskontro, purring), `regnmed
   abonnement`/`abonnement-faktura` CLI + prod-only CronJob
   (REGNMED_DRIFT_ORGNR). INGEN betalingsleverandør i v1 (faktura+KID);
   tier 2 = KORT via Stripe (bekreftet 2026-07-28 — firmakort er
   normen for SaaS), Vipps ev. tilleggs-skinne senere.
   `/me` bærer status; portalbanner i skallet varsler — serveren
   sperrer. Byråavtaler/moduler bevisst utsatt (dokumentert retning).
54. ✅ Kortskinnen (docs/abonnement.md §5, closed #74): kort er
   STANDARDVEIEN fra dag én — faktura krever at noen SER innbetalingen
   (bankfiler manuelle til bank-API finnes), webhook gjør ikke.
   Stripe, men ALDRI Stripe Billing: vår fakturamotor er autoritativ,
   PSP-en er bare en raskere vei til «betalt» på samme reskontropost —
   derfor utskiftbar (Nets Easy/Dintero er byttekandidater ved volum).
   `regnmed-gov::stripe` hand-rolled (kunde, Checkout setup-modus,
   off-session PaymentIntent m/ FAKTURA-ID SOM IDEMPOTENSNØKKEL,
   webhook-verifisering m/ egen HMAC-SHA256 testet mot RFC 4231 + begge
   retninger så testene kan signere). Migration 0043: `betalingskort`
   (referanser + brand/last4 — kortnummer finnes aldri hos oss),
   `kortbetaling` insert-only m/ UNIK payment_intent = webhook-replay
   er no-op. Webhooken bokfører 1570 Kortoppgjør mot 1500 m/ part +
   reskontro-match i ÉN tx m/ loggraden; feilede trekk logges (#75 tar
   oppfølgingen). Selvbetjent i portalen: Legg til kort (hosted
   checkout) + Start abonnement (kort-først, SELSKAP_ADMIN — virker
   også sperret). Config STRIPE_SECRET_KEY+STRIPE_WEBHOOK_SECRET
   (begge eller ingen) + REGNMED_DRIFT_ORGNR på API-et; uten = skinnen
   AV og portalen sier det. Live-verifisering venter på Stripe-konto.
   **Next:** Maskinporten (awaiting Skatteetaten scope grant,
   docs/gov.md),
   RF-1086 transaksjonstypekoder + innsending (#43-oppfølger),
   EHF-transport via aksesspunkt, API-tier per leverandør
   (#19-oppfølger), OCR-sidecar (#34-oppfølger).
   NOTE: run `cargo fmt --all` before every commit — CI gates on
   `cargo fmt --all --check` (learned 2026-07-25 after three red
   runs).
4. Portal UI, then marketplace features (BRREG onboarding, Finanstilsynet
   autorisasjon checks, accountant directory). Payroll (a-melding)
   deliberately deferred for years.
   **UI stack decision (2026-07-22): Tailwind v4 + daisyUI 5 across both
   sites.** **REVIDERT 2026-07-28 (#76): portalen migreres til Svelte 5
   (runes), Vite-bygget ren SPA — ingen SvelteKit-server/SSR. Alt annet
   består: Tailwind/daisyUI + temakontrakten, kompilert dist/ embeddes i
   binæren (app.css-presedensen: dist sjekkes inn, Rust-bygget trenger
   aldri node), én origin, PWA. Inkrementelt: ny app på /ny til
   seksjonsparitet, så flippes / — planen sto i #76. **FERDIG
   2026-07-29: Svelte-appen ER portalen.** ui/portal/ (Vite + Svelte 5
   runes + Tailwind/daisyUI), alle seksjonene + byråvisningen portert,
   dist sjekkes inn (scripts/build-portal.sh) og embeddes med
   include_dir — cargo trenger aldri node. Den rammeverksfrie portalen
   er slettet (app.js/theme.js/app.css/build-css.sh/input.css);
   themes.css flyttet til ui/portal/ fordi daisyUI slås opp mot
   nærmeste node_modules. /assets/* er immutable, ukjent asset gir 404
   (ikke appen), /ny redirecter til /. sw.js uttrykker skall-regelen
   som ADRESSER (/assets/* + PWA-filene) siden filnavnene er hashet —
   hovedboken caches fortsatt aldri. Vakter: dist-budsjett i
   frugality.sh + CI-jobb «portal» som feiler hvis innsjekket dist
   avviker fra kilden.** Themes are daisyUI themes (user-selectable, third-party
   authorable as single CSS blocks); the theme contract and canonical
   theme definitions live in `../regnid/ui/themes.css` — the portal UI
   must reuse the same theme names/blocks so a user's theme feels
   identical on both sites, but store its own per-user preference (never
   sync UI preferences through the IdP or tokens).
