# Deployment

One topology, described once, deployed as overlays:

```
deploy/base    the shared manifests: Postgres 18, NATS, regnid (+ mail
               worker), regnmed-api (migrate init container, nightly
               anchor CronJob)
deploy/local   k3d in colima, *.localhost, no TLS — the integration
               proving ground (scripts/dev-cluster.sh, 2 GB VM)
deploy/prod    real domains, TLS, secrets out of git, backups with
               restore-verification, TSA-witnessed anchoring
```

`kubectl kustomize deploy/<overlay>` renders either; the restructure
kept the local render byte-identical, so dev-cluster.sh is unchanged.

## Production checklist (deploy/prod)

1. **Pin images.** Build with `scripts/build-images.sh`, push to your
   registry, set the two `newTag` values in
   `deploy/prod/kustomization.yaml`. Never `:dev` in production.
2. **Hosts.** Three of them, and they mirror the local cluster:

   | Host | Serves | Ingress |
   | --- | --- | --- |
   | `regnmed.no` | the portal — what a human opens | `regnmed-portal` |
   | `api.regnmed.no` | the API — what an integration calls | `regnmed-api` |
   | `id.regnmed.no` | the IdP | `regnid` |

   The first two point at the **same service**: one binary serves both
   the SPA and the API (docs/portal.md). That is deliberate, and it is
   what keeps the portal's own calls same-origin — the app asks for
   `/companies/…` relative to wherever it was loaded, so a human on
   `regnmed.no` never makes a cross-origin request. No CORS, and
   `connect-src 'self'` in the CSP (docs/auth.md §9) keeps meaning what
   it says. Split them onto different services and both stop holding.

   Edit the hostnames in `deploy/prod/ingress.yaml`, the matching
   `ISSUER`/`OIDC_ISSUER` values in `deploy/prod/patches/` — the issuer
   URL the browser sees must be the one the pods see — and
   `PORTAL_BASE_URL`, which is the **portal** host, since it becomes the
   link in the invitation e-mail (#66). Register `https://regnmed.no/callback`
   as a redirect URI on the portal's OIDC client; add the API host too if
   the app should be openable there as well.
3. **TLS.** Install cert-manager, edit the e-mail in
   `cert-issuer.yaml`; Let's Encrypt HTTP-01 through Traefik issues and
   renews the certificates.
4. **Secrets — before the first apply, never in git:**

   ```sh
   kubectl -n regnmed create secret generic db-credentials \
     --from-literal=password='<strong password>' \
     --from-literal=regnmed-url='postgres://regnmed:<pw>@postgres:5432/regnmed' \
     --from-literal=regnid-url='postgres://regnmed:<pw>@postgres:5432/regnid' \
     --from-literal=restore-check-url='postgres://regnmed:<pw>@postgres:5432/regnmed_restore_check'
   ```

   Every DATABASE_URL/POSTGRES_PASSWORD in the prod render comes from
   this secret; the rendered YAML contains no credential (usernames and
   the OIDC audience are the only literals).
5. **Abonnementsfakturering** (docs/abonnement.md): onboard
   driftsselskapet i regnmed (BRREG, som alle andre) og sett dets orgnr
   som `REGNMED_DRIFT_ORGNR` i `deploy/prod/abonnement-faktura.yaml` —
   CronJob-en er prod-only (backup.yaml-mønsteret) fordi lokalklyngen
   ikke har noe driftsselskap. Kortskinnen (#74): legg
   `STRIPE_SECRET_KEY` og `STRIPE_WEBHOOK_SECRET` i en secret
   out-of-band (aldri i git) og sett dem + `REGNMED_DRIFT_ORGNR` på
   regnmed-api og CronJob-en; pek Stripes webhook på
   `https://<api-host>/stripe/webhook`.
6. `kubectl apply -k deploy/prod`.

## Pod hardening

Every container runs unprivileged, with no capabilities and a default
seccomp profile: `allowPrivilegeEscalation: false`,
`capabilities: drop [ALL]`, `seccompProfile: RuntimeDefault`. Beyond
that, the containers differ, and the differences are the interesting part.

| | runs as | root fs |
| --- | --- | --- |
| regnmed-api, regnid, mail worker, every CronJob | 65532 (nonroot) | read-only |
| postgres | 70 (`postgres`) | writable |
| nats | 65532 | read-only |

Our own images are distroless `:nonroot`, so they already had a nonroot
USER — but **an image saying so is not the same as the cluster requiring
it**, and nothing was requiring it. `runAsNonRoot` is what makes a future
image change fail loudly instead of quietly running as root.

`postgres` and `nats` were genuinely running as **root** until this was
set; both images drop privileges themselves, or would have if asked, and
neither was being asked. Postgres needs `fsGroup: 70` so the data
directory is owned by the user it now runs as, and keeps a **writable
root filesystem** on purpose: it writes its socket to
`/var/run/postgresql` and temp files to `/tmp`, neither a volume.
Turning those into emptyDirs to win the flag would add moving parts for
no real gain — the container is already unprivileged and capability-free.

`nats` gained `-m 8222` and a readiness probe on `/healthz`. Without one,
a client could be routed to a NATS that had not finished opening its
JetStream store. The mail worker deliberately has **no** probe; the
manifest explains why.

## Backups — restored weekly, or they don't count

`deploy/prod/backup.yaml`:

- **Nightly** `pg_dump` (custom format) of both databases to the
  backup PVC, pruned after 14 days.
- **Weekly restore-verification**: the newest dump is restored into a
  scratch database and `regnmed verify-ledger` re-walks every hash
  chain **in the restored copy** — including the anchor checks. This
  proves, unattended, that the backup restores *and* that the restored
  ledger is untampered. A backup that has never been restored is a
  hope, not a backup.

The same drill runs anywhere via `scripts/backup-verify.sh`
(`DATABASE_URL=… scripts/backup-verify.sh`). It has been exercised both
ways: a clean ledger passes; a database containing forged anchor rows
fails with the tampering named. Copy the backup PVC off-cluster (object
storage, another site) — a backup next to its database shares its
fate.

**Growth path, deliberate:** when RPO of minutes (not a day) is
required, move Postgres to the CloudNativePG operator with WAL
archiving to object storage — true PITR. The dump+verify drill stays
even then; PITR replaces the nightly granularity, not the verification.

## Observability, within the frugality budget

No metrics stack by default — the budget (docs/frugality.md) is spent
on the product. What production runs on:

- **Probes**: `/health` readiness on the API; pg_isready on Postgres.
- **Integrity monitoring is the observability that matters here**: the
  nightly anchor CronJob (every root witnessed via `ANCHOR_TSA_URL`)
  and the weekly backup-verification are both *checks that fail loudly
  in `kubectl get jobs` / alerting on failed Jobs* — they watch the one
  thing this system promises, the ledger.
- **Logs**: `kubectl logs`; ship them with the cluster's collector if
  one exists. A `/metrics` endpoint is a conscious later addition, and
  the frugality gate will price it.

## Deliberately not yet

Multi-node/HA Postgres, CloudNativePG (above), NetworkPolicies,
autoscaling — added when a real load or a real requirement asks, each
priced against the frugality budget.
