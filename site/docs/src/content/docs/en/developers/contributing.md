---
title: Contributing
description: How to contribute to regnmed. CLA, the contributor offer, test and language rules.
sidebar:
  order: 11
---

Contributions are welcome. The code is at [github.com/BogenTech/regnmed](https://github.com/BogenTech/regnmed).

## CLA

regnmed is AGPL-3.0, and the maintainer keeps the option to license on other terms. To keep that possible, **every contributor must agree to the [CLA](https://github.com/BogenTech/regnmed/blob/main/CLA.md)** before the contribution can be merged. In short, you keep the copyright, and you give the maintainer a broad licence to use, modify and relicense the contribution. You also confirm that it is your own work.

Put this line in the description of your first pull request:

> I have read the CLA and agree to its terms.

## Contributor offer

The CLA licence is the same for everyone. What is negotiated per contribution, by scope and value, is **your own subscription to the hosted service**: price (including free), plan and duration. Contact the maintainer before or after. Self-hosting is already free under the AGPL, so the offer concerns the service we actually charge for.

## How it works

1. Fork and create a feature branch.
2. Make the changes in small, clear commits.
3. Open a pull request that says what and why.
4. Include the CLA line in the first one.

Every change that goes into `main` is reviewed before it is merged. Each finding is fixed in the same PR or named as deliberately deferred, with the reason. A green CI is not a review.

## Build and test

```sh
docker compose up -d                 # Postgres 18 on port 5433, or scripts/dev-db.sh
cp .env.example .env
cargo run -p regnmed-cli -- migrate
cargo test                           # unit tests, no database needed
cargo nextest run --workspace        # the whole suite (needs DATABASE_URL), about 3.6 times faster
cargo fmt --all                      # CI fails on cargo fmt --all --check
```

Integration tests skip politely when `DATABASE_URL` is missing, and run for real in CI against Postgres 18. The API integration tests are **grouped** in `crates/regnmed-api/tests/grupper/` ("grupper" = groups): a new test belongs in an existing group, not in a new file in `tests/`.

What requires a test in the same commit: domain invariants (money, hashing, double-entry), general-ledger immutability, access boundaries and export formats. An authorization test that has not been seen to fail is not verified.

:::caution[The frozen hash]
`golden_hashes_never_change` in `regnmed-core::hash` pins the exact digest of the canonical serialization. If it fails, the change breaks verification of every deployed general ledger. The format can only be versioned, never edited. The same goes for the anchoring format.
:::

## Architecture rules you must know

- **Money is integer øre** (`Ore(i64)`), never floats. A balance is always `SUM(amount_ore)`, never stored state.
- **The general ledger is append-only**, enforced in domain, database and cryptography.
- **Migrations are append-only.** Never edit an applied migration.
- **`regnmed-core` has no I/O or database dependencies**, ever.
- **Authorization lives in regnmed's database, not in tokens.** IdP-specific details are never baked in.
- **API first.** Everything a user can do is an authenticated endpoint. The CLI is for operations only.

## Language rules

**Everything internal is English. Norwegian only where a user reads it.**

| Where | Language |
| --- | --- |
| Identifiers, database tables and columns, CLI commands, comments, test names, commit messages | English |
| Logs and error messages an operator reads | English |
| Text a user reads (the portal, error messages in the UI), prompts to a model | Norwegian |
| `docs/` (audit-facing) | Norwegian |

Exceptions: **applied migrations are append-only**, so an existing column keeps its name, and you map it to an English field in the code. **The wire format stays fixed**: JSON keys (`orgnr`, `termin`, `prosjekt`) are pinned with `#[serde(rename)]` and are not changed by a refactor, since the portal and customers' integrations read them. CLI command names are also pinned by a test (`the_ops_interface_has_not_moved`).

If you rename an identifier mechanically, there are tools that verify that strings, wire names and CLI surfaces are unchanged (`scripts/rename-identifiers.py`, `scripts/wire-names.py`, `scripts/pin-wire.py`).

## Don't

- Don't put secrets in the repo, in any form. See [Self-hosting](/en/developers/self-hosting/).
- Don't edit an applied migration.
- Don't add a new `tests/*.rs` file for a single test.
