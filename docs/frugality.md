# The frugality budget

Competing on the least resources is a product principle (ROADMAP.md):
regnmed's pitch includes running a full accounting platform where
others need a rack. Principles that aren't enforced decay, so this one
is a CI gate — **a service that grows fat fails the build** (issue
#28, `scripts/frugality.sh`, the `frugality` job in
`.github/workflows/ci.yml`).

## What is measured, against what budget

| Measure | Budget | Today (2026-07-23) |
| --- | --- | --- |
| `regnmed-api` release binary | 24 MB | 11 MB |
| `regnmed` CLI release binary | 20 MB | 8 MB |
| `regnmed-api` peak RSS under load | **64 MB** | 11 MB |

The RSS measurement is real, not synthetic: the release binary runs
against a real Postgres, serves several hundred requests across the
hot unauthenticated paths (portal static, config, the public anchors
feed) plus rejected bearer tokens (the auth path), and its high-water
mark (Linux `VmHWM`) must stay under budget.

The RSS budget **equals the container limit** in
`deploy/local/regnmed-api.yaml` — if the gate passes, the pod cannot be
OOM-killed at that limit. The binary budgets are ~2× measured reality:
tight enough to catch an accidentally vendored embedding model or a
debug-symbol regression, loose enough for normal growth.

## Raising a budget

Budgets may be raised — consciously. The change goes in
`scripts/frugality.sh` (and `deploy/local` for the RSS limit) in its
own commit that says why the growth is worth it. The gate exists to
force that sentence to be written, not to forbid growth.

## Running locally

```sh
scripts/dev-db.sh                       # or docker compose up -d
DATABASE_URL=postgres://… scripts/frugality.sh
```

Works on macOS too (falls back to polling `ps` for RSS, which under-
reports slightly versus `VmHWM` — CI's Linux number is authoritative).
