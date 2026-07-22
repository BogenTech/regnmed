# Identity and authorization

Two deliberately separated concerns:

- **Identity** (who you are): proven by an OIDC token from the IdP.
- **Authorization** (what you may do, for which company): decided by
  regnmed's own database. Never carried in tokens — an accountant with 60
  clients cannot meaningfully carry that in a JWT, and access changes
  must take effect without re-login.

## Identity: OIDC relying party only

The IdP is **regnid** (sibling repo, our Rust port of networco-id).
regnmed validates RS256 tokens against a configured issuer/JWKS
(`crates/regnmed-api/src/auth.rs`: `Verifier` + the `AuthPerson`
extractor — adding `AuthPerson` as a handler argument protects the
route). regnmed never bakes in IdP specifics; any spec-compliant issuer
works. Config: `OIDC_ISSUER`, optional `OIDC_AUDIENCE`,
`OIDC_JWKS_FILE` (dev/tests: static JWKS, signatures still validated).

Rejected with 401, verified by tests: missing/garbage tokens, tokens
signed by the wrong key, expired tokens, wrong audience.

## Authorization: the engagement model

Migration 0005 (`person`, `firm`, `firm_member`, `company_member`,
`engagement`):

```
person ──── company_member ────────────────► company   ("direkte")
   │
   └─────── firm_member ──► firm ── engagement (oppdrag) ──► company
```

- An **engagement** (oppdrag) is the first-class relationship between a
  regnskapsfører-/revisorfirma and a client company, with scope
  (regnskap/revisjon) and validity. Revisor engagements are read-only +
  chain verification.
- `/me` resolves token → person (JIT-provisioned on first login) → all
  companies the person may act for, each with its access level and the
  path it came through (`via` = firm name or "direkte").
- This mirrors Altinn's delegation model (see gov.md), which will let
  government-side delegation and regnmed-side engagements stay aligned.

## Per-company guard on API routes

Every company-scoped endpoint resolves the caller's access with
`regnmed_db::company_access(person, company)` before touching data. No
path to the company yields **404, not 403** — a caller without access
must not learn that the company exists. All access levels (admin /
bokforing / les — revisor included) may read reports, since reports
never mutate the ledger; mutating endpoints will require the appropriate
level when they arrive.

## Where it is tested

`crates/regnmed-api/tests/me_endpoint.rs` (real Postgres, also CI): a
locally generated JWKS signs real RS256 tokens; a seeded
firm-with-engagement plus a direct membership resolve to exactly the
expected company list; the forged/expired/wrong-audience matrix is
rejected.
