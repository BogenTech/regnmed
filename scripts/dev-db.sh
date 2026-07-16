#!/usr/bin/env bash
# Start a local dev Postgres on port 5433 without Docker, using Homebrew
# postgresql@18 (brew install postgresql@18). Data lives in .dev/pgdata,
# which is gitignored. Dev only: local trust auth, TCP on localhost.
set -euo pipefail

# Avoid macOS locale issues (initdb errors, "postmaster became
# multithreaded during startup") regardless of the caller's LANG/LC_*.
export LC_ALL=C

PG_BIN="$(brew --prefix postgresql@18)/bin"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA_DIR="$REPO_ROOT/.dev/pgdata"
PORT=5433

if [ ! -d "$DATA_DIR" ]; then
    mkdir -p "$DATA_DIR"
    "$PG_BIN/initdb" -D "$DATA_DIR" -U regnmed --auth=trust \
        --encoding=UTF8 --locale=C >/dev/null
fi

if "$PG_BIN/pg_ctl" -D "$DATA_DIR" status >/dev/null 2>&1; then
    echo "dev postgres already running on port $PORT"
else
    "$PG_BIN/pg_ctl" -D "$DATA_DIR" -o "-p $PORT" -l "$DATA_DIR/server.log" start >/dev/null
    echo "dev postgres started on port $PORT"
fi

"$PG_BIN/createdb" -h localhost -p "$PORT" -U regnmed regnmed 2>/dev/null || true
echo "stop with: $PG_BIN/pg_ctl -D $DATA_DIR stop"
