-- Trekkpålegg fetched from Skatteetaten (docs/trekkpalegg.md §2, #334).
--
-- Three things, all insert-only:
--
-- 1. `garnishment_fetch`: one row per call, written BEFORE the call, so the
--    terms' §7.1 register shows every request, also one that fails or is
--    lost on the way (the skattekort pattern, migration 0079).
-- 2. `garnishment_fetch_outcome`: what came back. On success the trekkids
--    the list held: the list is the whole list (§2 step 1), so an active
--    order missing from the newest successful one is an anomaly and is not
--    withheld.
-- 3. `garnishment_fetched`: every version of every order, verbatim, keyed
--    on (trekkid, trekkversjon) per company. A version regnmed already has
--    is skipped. The employee is NOT stored: the link is `skyldner` against
--    the employee register, derived when read, so an employee registered
--    after the order arrived is found.
--
-- And `payroll_garnishment` (0110) may now name a fetched version instead
-- of a registered one; `version` then holds the trekkversjon.

create table garnishment_fetch (
    id            uuid primary key,
    company_id    uuid not null references company (id),
    requested_by  text not null check (requested_by <> ''),
    created_at    timestamptz not null default now()
);
create index garnishment_fetch_company_idx on garnishment_fetch (company_id, created_at desc);

create table garnishment_fetch_outcome (
    fetch_id    uuid primary key references garnishment_fetch (id),
    ok          boolean not null,
    -- Success: the trekkids in the list. Failure: Skatteetaten's answer or
    -- the transport error, verbatim (behind LONN_LES, like the orders).
    listed      text[],
    detail      text,
    created_at  timestamptz not null default now(),
    check (ok = (listed is not null)),
    check (ok or detail <> '')
);

create table garnishment_fetched (
    id             uuid primary key,
    company_id     uuid not null references company (id),
    fetch_id       uuid not null references garnishment_fetch (id),
    trekkid        text not null check (trekkid <> ''),
    trekkversjon   integer not null check (trekkversjon >= 1),
    skyldner       text not null check (skyldner ~ '^[0-9]{11}$'),
    opprettet      timestamptz not null,
    -- The order as it came: periods, sizes, status, payment details.
    raw            jsonb not null,
    created_at     timestamptz not null default now(),
    unique (company_id, trekkid, trekkversjon)
);
create index garnishment_fetched_skyldner_idx on garnishment_fetched (company_id, skyldner);

create trigger garnishment_fetch_append_only
    before update or delete on garnishment_fetch
    for each row execute function forbid_ledger_mutation();
create trigger garnishment_fetch_no_truncate
    before truncate on garnishment_fetch
    for each statement execute function forbid_ledger_mutation();
create trigger garnishment_fetch_outcome_append_only
    before update or delete on garnishment_fetch_outcome
    for each row execute function forbid_ledger_mutation();
create trigger garnishment_fetch_outcome_no_truncate
    before truncate on garnishment_fetch_outcome
    for each statement execute function forbid_ledger_mutation();
create trigger garnishment_fetched_append_only
    before update or delete on garnishment_fetched
    for each row execute function forbid_ledger_mutation();
create trigger garnishment_fetched_no_truncate
    before truncate on garnishment_fetched
    for each statement execute function forbid_ledger_mutation();
grant select, insert on garnishment_fetch, garnishment_fetch_outcome, garnishment_fetched
    to regnmed_app;

-- A run's line names exactly one source.
alter table payroll_garnishment
    alter column garnishment_id drop not null,
    add column fetched_id uuid references garnishment_fetched (id),
    add constraint payroll_garnishment_one_source
        check ((garnishment_id is null) <> (fetched_id is null));

-- The daily fetch is a scheduled background job (docs/jobs.md).
alter table job drop constraint job_kind_check;
alter table job add constraint job_kind_check check (kind in (
    'saft_import', 'audit_report', 'bank_sync',
    'peppol_lookup', 'invoice_dispatch', 'peppol_sender_setup',
    'vat_filing_submit', 'garnishment_fetch'));
