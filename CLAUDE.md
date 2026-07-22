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
     `regnmed verify-ledger` re-walks chains from genesis. Planned: external
     anchoring of chain heads so even DBA-level tampering is provable.
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
  token → companies + access. Config: `OIDC_ISSUER`, optional
  `OIDC_AUDIENCE`, `OIDC_JWKS_FILE` (dev/tests: static JWKS, signatures
  still validated), `BIND_ADDR`.
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
```

Norwegian domain terms are used deliberately (bilag, hovedbok, oppdrag,
kontoplan NS 4102, SAF-T VAT codes); don't translate them away in code or docs.

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
cargo. Manifests: `deploy/local/` (kustomize). Not yet: TLS (mkcert),
multi-node, operators — add when a concept needs them.

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
   **Next:** MVA codes end-to-end (then EHF/Peppol, bank reconciliation).
4. Portal UI, then marketplace features (BRREG onboarding, Finanstilsynet
   autorisasjon checks, accountant directory). Payroll (a-melding)
   deliberately deferred for years.
   **UI stack decision (2026-07-22): Tailwind v4 + daisyUI 5 across both
   sites.** Themes are daisyUI themes (user-selectable, third-party
   authorable as single CSS blocks); the theme contract and canonical
   theme definitions live in `../regnid/ui/themes.css` — the portal UI
   must reuse the same theme names/blocks so a user's theme feels
   identical on both sites, but store its own per-user preference (never
   sync UI preferences through the IdP or tokens).
