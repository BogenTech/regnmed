#!/usr/bin/env bash
# Backup + restore-verification drill (docs/deploy.md) — the same logic
# the weekly prod CronJob runs (deploy/prod/backup.yaml), runnable
# against any Postgres: dump the regnmed database, restore it into a
# scratch database, and re-walk every hash chain in the RESTORED copy
# with `regnmed verify-ledger`. Proves the backup restores AND that the
# restored ledger is untampered. A backup that has never been restored
# is a hope, not a backup.
#
# Usage: DATABASE_URL=postgres://user:pass@host:port/regnmed scripts/backup-verify.sh
set -euo pipefail
cd "$(dirname "$0")/.."

: "${DATABASE_URL:?DATABASE_URL must point at the regnmed database}"
SCRATCH_DB=regnmed_restore_check
ADMIN_URL=${DATABASE_URL%/*}/postgres
SCRATCH_URL=${DATABASE_URL%/*}/$SCRATCH_DB

DUMP=$(mktemp -t regnmed-backup-XXXXXX.dump)
trap 'rm -f "$DUMP"; psql "$ADMIN_URL" -qc "drop database if exists $SCRATCH_DB" 2>/dev/null || true' EXIT

echo "==> dumping"
pg_dump --format=custom --file="$DUMP" "$DATABASE_URL"
echo "    $(du -h "$DUMP" | cut -f1) dump written"

echo "==> restoring into $SCRATCH_DB"
psql "$ADMIN_URL" -qc "drop database if exists $SCRATCH_DB"
psql "$ADMIN_URL" -qc "create database $SCRATCH_DB"
pg_restore --no-owner --dbname="$SCRATCH_URL" "$DUMP"

echo "==> verifying every hash chain in the restored copy"
DATABASE_URL="$SCRATCH_URL" cargo run -q --release -p regnmed-cli -- verify-ledger | tail -3

echo "==> backup verified: it restores, and the restored ledger is untampered"
