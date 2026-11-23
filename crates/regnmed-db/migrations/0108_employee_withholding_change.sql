-- A person setting an employee's withholding by hand (docs/lonn.md). It
-- decides how much tax leaves the employee's pay, so who changed it, from
-- what, to what and why must be answerable afterwards. A fetched skattekort
-- has its own trail in `tax_card`.
create table employee_withholding_change (
    id                          uuid primary key default gen_random_uuid(),
    company_id                  uuid not null references company (id),
    employee_id                 uuid not null references employee (id),
    old_type                    text not null,
    old_percent_bp              integer,
    old_table                   integer,
    old_twelve_months           boolean not null,
    old_tax_free_card_limit_ore bigint,
    new_type                    text not null check (new_type in ('prosent', 'tabell', 'ingen')),
    new_percent_bp              integer check (new_percent_bp between 0 and 10000),
    new_table                   integer,
    reason                      text not null check (btrim(reason) <> ''),
    changed_by                  text not null,
    created_at                  timestamptz not null default now()
);

create index employee_withholding_change_idx
    on employee_withholding_change (employee_id, created_at desc);

create trigger employee_withholding_change_append_only
    before update or delete on employee_withholding_change
    for each row execute function forbid_ledger_mutation();
create trigger employee_withholding_change_no_truncate
    before truncate on employee_withholding_change
    for each statement execute function forbid_ledger_mutation();

grant select, insert on employee_withholding_change to regnmed_app;
