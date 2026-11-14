-- Money out for expense claims (docs/utlegg.md «Utbetaling»).
--
-- 1. Kjøregodtgjørelse is paid WITH PAY. The part above the trekkfri rate
--    is trekkpliktig and triggers arbeidsgiveravgift, and only a payroll
--    run can withhold tax; both parts are reported in the a-melding for
--    the month they are paid. A run therefore settles the employee's
--    approved kjøring claims: payroll_expense records which claim a run
--    paid and how it splits for the a-melding -- a snapshot, like every
--    payroll row, so the melding can be rebuilt from the run forever.
-- 2. Approved receipt claims (utlegg) go into the betalingsliste. A
--    payment item now settles EITHER an open supplier post OR a claim.
-- 3. A billable claim is rebilled on the project customer's invoice,
--    once: expense_invoice records which invoice took it and how --
--    resold with VAT, or passed on as a disbursement outside the VAT
--    base (mval. § 4-1 second paragraph letter a).

alter table payroll_run add column allowance_ore bigint not null default 0;
alter table payroll_line
    add column allowance_ore bigint not null default 0,
    add column taxable_allowance_ore bigint not null default 0;

create table payroll_expense (
    id                     uuid primary key,
    run_id                 uuid not null references payroll_run (id),
    employee_id            uuid not null references employee (id),
    -- A claim is paid once: a second run cannot take it again.
    expense_id             uuid not null unique references expense (id),
    -- Kilometergodtgjørelse bil, or another vehicle (a separate code).
    car                    boolean not null,
    km                     bigint not null check (km > 0),
    -- km × passengers: the supplement is paid per passenger per km.
    passenger_km           bigint not null check (passenger_km >= 0),
    vehicle_tax_free_ore   bigint not null check (vehicle_tax_free_ore >= 0),
    passenger_tax_free_ore bigint not null check (passenger_tax_free_ore >= 0),
    taxable_ore            bigint not null check (taxable_ore >= 0),
    created_at             timestamptz not null default now()
);

create index payroll_expense_run_idx on payroll_expense (run_id, employee_id);

create trigger payroll_expense_append_only
    before update or delete on payroll_expense
    for each row execute function forbid_ledger_mutation();
create trigger payroll_expense_no_truncate
    before truncate on payroll_expense
    for each statement execute function forbid_ledger_mutation();

grant select, insert on payroll_expense to regnmed_app;

alter table payment_run_item alter column entry_id drop not null;
alter table payment_run_item
    add column expense_id uuid references expense (id),
    add constraint payment_run_item_target_check
        check ((entry_id is null) <> (expense_id is null));

create index payment_run_item_expense_idx on payment_run_item (expense_id)
    where expense_id is not null;

create table expense_invoice (
    -- A claim is rebilled once.
    expense_id uuid primary key references expense (id),
    invoice_id uuid not null references invoice (id),
    treatment  text not null check (treatment in ('videresalg', 'utlegg')),
    created_by text not null check (created_by <> ''),
    created_at timestamptz not null default now()
);

create index expense_invoice_invoice_idx on expense_invoice (invoice_id);

create trigger expense_invoice_append_only
    before update or delete on expense_invoice
    for each row execute function forbid_ledger_mutation();
create trigger expense_invoice_no_truncate
    before truncate on expense_invoice
    for each statement execute function forbid_ledger_mutation();

grant select, insert on expense_invoice to regnmed_app;
