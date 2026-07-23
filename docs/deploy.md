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
2. **Hosts.** Edit the two hostnames in `deploy/prod/ingress.yaml` and
   the matching `ISSUER`/`OIDC_ISSUER` values in `deploy/prod/patches/`
   — the issuer URL the browser sees must be the one the pods see.
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
5. `kubectl apply -k deploy/prod`.

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
