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

regnmed is **dual-licensed**:

- **[AGPL-3.0](LICENSE)** — free to use, provided you comply with the AGPL terms. Note that AGPL requires you to make your complete corresponding source code available to all users, including users who interact with the software **over a network** (e.g. SaaS).
- **[Commercial license](COMMERCIAL-LICENSE.md)** — for use in proprietary products or hosted services without AGPL obligations.

This project is *source available* under a copyleft license; commercial use without AGPL compliance requires a paid license.

## Contributing

Contributions are welcome! All contributors must agree to our [CLA](CLA.md) — see [CONTRIBUTING.md](CONTRIBUTING.md). Significant contributors may qualify for a **free or discounted commercial license**.
