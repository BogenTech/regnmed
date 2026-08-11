# regnmed

Regnskapssystem for det norske markedet, skrevet i Rust. Hovedboken er en
append-only, hash-kjedet journal: hvert bilag lagrer `SHA-256(prev_hash ||
innhold)`, databasen avviser UPDATE/DELETE på historikk, og `regnmed
verify-ledger` re-verifiserer hele kjeden fra genesis — manipulering er
detekterbar, også for noen med full databasetilgang.

## Development

```sh
# Dev database (PostgreSQL on port 5433) — pick one:
docker compose up -d        # with Docker
scripts/dev-db.sh           # without Docker (brew install postgresql@18)

cp .env.example .env

cargo run -p regnmed-cli -- migrate        # run migrations
cargo run -p regnmed-cli -- demo           # post demo vouchers + verify chain
cargo run -p regnmed-cli -- verify-ledger  # re-verify all hash chains
cargo test                                 # unit tests (no database needed)
cargo nextest run --workspace              # full suite (needs DATABASE_URL)
```

Workspace layout:

- `crates/regnmed-core` — domain model: money (integer øre), vouchers,
  double-entry validation, canonical chain hashing. No I/O.
- `crates/regnmed-db` — PostgreSQL persistence: migrations (sqlx), posting
  transaction, chain verification.
- `crates/regnmed-api` — HTTP API (axum) with OIDC relying-party auth:
  tokens are validated against a configured issuer (JWKS), identity only —
  authorization (person → firm → engagement → company) is resolved from the
  database. `GET /me` lists the companies the caller may act for.
- `crates/regnmed-cli` — `regnmed` admin binary: `migrate`, `verify-ledger`,
  `demo`.

## License

regnmed is licensed under the **[GNU AGPL-3.0](LICENSE)**. The source is public
so that anyone — customers, revisorer, security researchers — can read exactly
how the ledger is built and check the guarantees it claims. That is the point:
don't trust us, verify.

**Customers of the hosted service need no license from us.** If you use
regnmed.no, you subscribe and that is all — nothing in this section applies to
you.

**If you self-host,** AGPL-3.0 applies: you are free to run and modify regnmed,
provided you make your complete corresponding source available to your users,
including users who interact with it **over a network**.

A separate [commercial license](COMMERCIAL-LICENSE.md) is available by agreement
for the unusual case where AGPL-3.0 does not work for you. It is not how regnmed
is sold — the product is the hosted service.

## Contributing

Contributions are welcome! All contributors must agree to our [CLA](CLA.md) —
see [CONTRIBUTING.md](CONTRIBUTING.md). Significant contributors may qualify for
a **free or discounted regnmed subscription**.
