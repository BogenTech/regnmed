-- Skattekort fetched from Skatteetaten (docs/lonn.md «Skattekort fra
-- Skatteetaten», #46).
--
-- An order is one call to bestillSkattekort; its answer is one row per
-- employee, the card verbatim. Both insert-only: what Skatteetaten said
-- about an employee, and when, is evidence of why the withholding was
-- what it was. Applying a card updates `employee` (the columns 0035
-- already lets the app update); `applied` here records THAT it did, and
-- `note` why it did not.
create table tax_card_order (
    id                uuid primary key,
    company_id        uuid not null references company (id),
    income_year       integer not null check (income_year between 2000 and 2100),
    -- bestillingsreferanse (BR…): what the answer is fetched with.
    reference         text not null unique check (reference <> ''),
    dialog_reference  text not null check (dialog_reference <> ''),
    employee_count    integer not null check (employee_count between 1 and 1000),
    requested_by      text not null check (requested_by <> ''),
    created_at        timestamptz not null default now()
);

create index tax_card_order_company_idx on tax_card_order (company_id, created_at desc);

create table tax_card (
    id            uuid primary key,
    order_id      uuid not null references tax_card_order (id),
    company_id    uuid not null references company (id),
    employee_id   uuid not null references employee (id),
    income_year   integer not null,
    -- resultatForSkattekort, e.g. skattekortopplysningerOK / ikkeSkattekort.
    result        text not null check (result <> ''),
    card_id       bigint,
    issued_date   date,
    -- The employee's entry in the answer, verbatim.
    card          jsonb not null,
    -- What was set on the employee, in 0035's own vocabulary; NULL when the
    -- card was not applied, and then `note` says why.
    applied             text check (applied in ('prosent', 'tabell', 'ingen')),
    applied_percent_bp  integer,
    applied_table       integer,
    note          text,
    created_at    timestamptz not null default now(),
    check (applied is not null or note is not null),
    unique (order_id, employee_id)
);

create index tax_card_employee_idx on tax_card (employee_id, created_at desc);

create trigger tax_card_order_append_only
    before update or delete on tax_card_order
    for each row execute function forbid_ledger_mutation();
create trigger tax_card_order_no_truncate
    before truncate on tax_card_order
    for each statement execute function forbid_ledger_mutation();
create trigger tax_card_append_only
    before update or delete on tax_card
    for each row execute function forbid_ledger_mutation();
create trigger tax_card_no_truncate
    before truncate on tax_card
    for each statement execute function forbid_ledger_mutation();

grant select, insert on tax_card_order to regnmed_app;
grant select, insert on tax_card to regnmed_app;
