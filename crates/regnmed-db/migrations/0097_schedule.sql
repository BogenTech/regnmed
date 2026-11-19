-- A general scheduler (docs/jobs.md, «Tidsplaner»): a schedule says WHEN a
-- kind of background job runs for a company; when it is due, the worker
-- creates an ordinary `job` row and the existing machinery runs it (claim,
-- heartbeat, retries, scale-out). The scheduler runs nothing itself, and
-- anything that can be a job can be scheduled. First consumer: the bank
-- connection's automatic fetch (docs/bank.md).
--
-- A schedule is configuration, so it is editable (times, active) and says
-- who changed it last; what actually ran is the job rows, which are the
-- history. `next_run_at` is operational state like `job.status`: the worker
-- advances it in the same transaction that creates the job.

-- The next of `times` (local Norwegian wall-clock times) strictly after
-- `after`. One definition, used when a schedule is saved and when it fires.
create function schedule_next(times time[], after timestamptz) returns timestamptz
language sql stable as $$
    select min(t)
    from (
        select ((d + tm) at time zone 'Europe/Oslo') as t
        from unnest(times) as tm,
             generate_series(0, 1) as off,
             lateral (select (after at time zone 'Europe/Oslo')::date + off as d) x
    ) c
    where t > after
$$;

create table schedule (
    id          uuid primary key default gen_random_uuid(),
    company_id  uuid not null references company (id),
    -- A job kind (regnmed-db::job::Kind). Which kinds may be scheduled, and
    -- how often, is decided in code (regnmed-db::schedule), not here.
    kind        text not null check (kind <> ''),
    -- Daily, at these local Norwegian times.
    times       time[] not null check (cardinality(times) between 1 and 24),
    active      boolean not null default true,
    next_run_at timestamptz not null,
    updated_by  text not null check (updated_by <> ''),
    updated_at  timestamptz not null default now(),
    created_at  timestamptz not null default now(),
    unique (company_id, kind)
);

create index schedule_due on schedule (next_run_at) where active;

grant select, insert on schedule to regnmed_app;
grant update (times, active, next_run_at, updated_by, updated_at) on schedule to regnmed_app;

create trigger schedule_no_delete
    before delete on schedule
    for each row execute function forbid_ledger_mutation();

-- A job may come from a schedule, and the bank fetch is a job kind.
alter table job add column schedule_id uuid references schedule (id);
alter table job drop constraint job_kind_check;
alter table job add constraint job_kind_check
    check (kind in ('saft_import', 'audit_report', 'bank_sync'));
create index job_schedule on job (schedule_id, created_at desc) where schedule_id is not null;
