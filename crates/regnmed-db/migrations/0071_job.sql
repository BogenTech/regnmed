-- Background jobs (docs/jobs.md): work too slow to hold a request open
-- for -- a SAF-T import of several years, the audit report over a large
-- ledger -- runs in `regnmed work`, which any number of replicas can run
-- side by side.
--
-- This table is the record; the JetStream queue is a doorbell (the same
-- split as the agent queue, 0069). A worker CLAIMS a job here with one
-- atomic update before it does anything, so a duplicated or redelivered
-- message, or a sweep running at the same moment, can never put two
-- workers on one job. A worker that dies stops heartbeating, and after a
-- minute another one may take the job over -- at most three attempts.
--
-- What was asked is immutable (column grants): kind, company, input and
-- who asked. Only the progress columns move. The input bytes go through
-- the blob store like every other document (0070): with an object store
-- configured the row keeps only the hash.
create table job (
    id                 uuid primary key,
    company_id         uuid        not null references company (id),
    kind               text        not null check (kind in ('saft_import', 'audit_report')),
    input_sha256       bytea       check (input_sha256 is null or octet_length(input_sha256) = 32),
    input              bytea,
    input_content_type text,
    created_by         text        not null check (created_by <> ''),
    created_by_person  uuid        references person (id),
    created_at         timestamptz not null default now(),
    status             text        not null default 'queued'
                       check (status in ('queued', 'running', 'done', 'failed')),
    attempts           int         not null default 0,
    claimed_by         text,
    heartbeat_at       timestamptz,
    started_at         timestamptz,
    finished_at        timestamptz,
    result             jsonb,
    error              text,
    check (input is null or input_sha256 is not null)
);

-- The sweep's query: what is waiting, oldest first.
create index job_open on job (created_at) where status in ('queued', 'running');
create index job_company on job (company_id, created_at desc);

grant select, insert on job to regnmed_app;
grant update (status, attempts, claimed_by, heartbeat_at, started_at, finished_at, result, error)
    on job to regnmed_app;
