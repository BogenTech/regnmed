# The frugality budget

Competing on the least resources is a product principle (ROADMAP.md):
regnmed's pitch includes running a full accounting platform where
others need a rack. Principles that aren't enforced decay, so this one
is a CI gate — **a service that grows fat fails the build** (issue
#28, `scripts/frugality.sh`, the `frugality` job in
`.github/workflows/ci.yml`).

## What is measured, against what budget

| Measure | Budget | Measured in CI (2026-11-12) |
| --- | --- | --- |
| `regnmed-api` release binary | 20 MB | 18 MB |
| `regnmed` CLI release binary | 12 MB | 10 MB |
| `regnmed-agent` release binary | 8 MB | 8 MB |
| `regnmed-api` peak RSS under load | **64 MB** | 13 MB |
| Portal first load, gzip -9 | 80 KB | 62 KB |
| — of which CSS, gzip -9 | 32 KB | 24 KB |

The RSS measurement is real, not synthetic: the release binary runs
against a real Postgres, serves several hundred requests across the
hot unauthenticated paths (portal static, config, the public anchors
feed) plus rejected bearer tokens (the auth path), and its high-water
mark (Linux `VmHWM`) must stay under budget.

The RSS budget **equals the container limit** in
`deploy/local/regnmed-api.yaml` — if the gate passes, the pod cannot be
OOM-killed at that limit. The binary budgets were set ~1.5× above measured reality:
tight enough to catch an accidentally vendored embedding model or a
debug-symbol regression, loose enough for normal growth.

### The job worker (not gated)

`regnmed work` (docs/jobs.md) has a 256 Mi container limit and no gate
yet. Measured 2026-11-09 on a release build: 11 MB idle, **55 MB peak**
while importing a 10 MB SAF-T file (10 429 vouchers). The limit allows
two such imports at once (`JOB_CONCURRENCY=2`) with room to spare. A gate
would need a SAF-T fixture of that size in CI; until then, re-measure
when a new job kind lands.

### The portal: first load, not the total

The portal (`ui/portal/dist`) is measured as **what a browser must
download before the first page can draw**: every `/assets/` file that
`index.html` names (the entry JS and the CSS, plus any `modulepreload`
Vite adds), each compressed with `gzip -9` — roughly what goes over the
wire. The file list is read from `index.html`, not from a glob, so a
renamed entry cannot slip past the gate.

The lazy-loaded section chunks (24 of them, ~109 KB gzipped on
2026-11-12) are deliberately **not** budgeted: a section is fetched when
it is opened, not before, and its size is already inside the
`regnmed-api` binary budget, which embeds the whole dist.

Until 2026-11-12 the gate summed the *uncompressed total* of the dist
(last budget 568 KB). That number grew with every feature — raised three
times in two weeks — and measured nothing a user waits for. The CSS keeps
its own line because it answers the first question when the portal
grows: CSS that grows without a theme change is a dependency nobody
meant to add.

## The gate must be able to fail

The CI step pipes the report into the job summary
(`scripts/frugality.sh | tee -a "$GITHUB_STEP_SUMMARY"`). Without
`pipefail` the step takes `tee`'s exit status and is always green — run
43253844495 printed `FAIL` for the portal and passed. The step therefore
declares `shell: bash`, which GitHub runs as `bash -eo pipefail`. Seen
to fail on 2026-11-12 with the budget lowered, and pass again restored.

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
