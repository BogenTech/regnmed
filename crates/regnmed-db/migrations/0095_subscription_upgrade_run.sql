-- An upgrade in the middle of a month that is already invoiced is billed the
-- price difference for that month (user decision 2026-11-18, docs/abonnement.md
-- §7): otherwise a company could upgrade on the 2nd and downgrade before the
-- next run, using the bigger package while paying for the smaller one.
--
-- One row per (company, month, target plan), written in the same transaction
-- as the switch and the difference invoice, so a retried request can never
-- bill the same upgrade twice. Insert-only, like the monthly run log.
create table subscription_upgrade_run (
    id         uuid primary key,
    company_id uuid not null references company (id),
    ar         int not null,
    maned      int not null check (maned between 1 and 12),
    from_plan  text not null check (from_plan <> ''),
    to_plan    text not null check (to_plan <> ''),
    invoice_id uuid not null,
    created_at timestamptz not null default now(),
    unique (company_id, ar, maned, to_plan)
);

grant select, insert on subscription_upgrade_run to regnmed_app;

create trigger subscription_upgrade_run_append_only
    before update or delete on subscription_upgrade_run
    for each row execute function forbid_ledger_mutation();
create trigger subscription_upgrade_run_no_truncate
    before truncate on subscription_upgrade_run
    for each statement execute function forbid_ledger_mutation();
