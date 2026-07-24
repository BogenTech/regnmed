-- Repeterende faktura (docs/faktura.md, #30).
--
-- A template is ORDINARY EDITABLE DATA — it is a plan, not evidence.
-- Nothing regnskapsmessig happens until generation, which creates a
-- normal invoice through the existing gap-free path (counter + KID +
-- posting + PDF in one transaction). The generation LOG is evidence:
-- insert-only, one row per attempt, and the partial unique index makes
-- generation idempotent per (template, period) even under concurrent
-- runs. Templates are deactivated, never deleted, once they have runs
-- (FK without cascade enforces it).

create table invoice_template (
    id            uuid primary key,
    company_id    uuid not null references company (id),
    party_id      uuid not null references party (id),
    intervall     text not null check (intervall in ('manedlig', 'kvartalsvis', 'arlig')),
    neste_dato    date not null,
    -- Inclusive; null = until deactivated.
    slutt_dato    date,
    forfall_dager integer not null default 14 check (forfall_dager between 0 and 365),
    -- Generation MARKS the invoice for sending (til_utsendelse on the
    -- run); the send itself stays an explicit human action.
    merk_utsendelse boolean not null default false,
    active        boolean not null default true,
    created_by    text not null check (created_by <> ''),
    created_at    timestamptz not null default now(),
    updated_at    timestamptz not null default now()
);

create index invoice_template_due
    on invoice_template (neste_dato) where active;

create table invoice_template_line (
    id             uuid primary key,
    template_id    uuid not null references invoice_template (id),
    line_no        integer not null check (line_no > 0),
    -- May contain {måned}/{år}, interpolated at generation.
    description    text not null check (description <> ''),
    account_number text not null,
    quantity_milli bigint not null check (quantity_milli <> 0),
    unit_price_ore bigint not null,
    vat_code       text references vat_code (code),
    avdeling       text,
    prosjekt       text,
    unique (template_id, line_no)
);

create table invoice_template_run (
    id              uuid primary key,
    template_id     uuid not null references invoice_template (id),
    -- Null when generation failed; the error is in detail.
    invoice_id      uuid references invoice (id),
    generated_for   date not null,
    til_utsendelse  boolean not null default false,
    detail          text,
    created_at      timestamptz not null default now()
);

-- Idempotence: at most one SUCCESSFUL generation per template/period.
create unique index invoice_template_run_once
    on invoice_template_run (template_id, generated_for)
    where invoice_id is not null;

create trigger invoice_template_run_append_only
    before update or delete on invoice_template_run
    for each row execute function forbid_ledger_mutation();
create trigger invoice_template_run_no_truncate
    before truncate on invoice_template_run
    for each statement execute function forbid_ledger_mutation();

grant select, insert, update on invoice_template to regnmed_app;
grant select, insert, update, delete on invoice_template_line to regnmed_app;
grant select, insert on invoice_template_run to regnmed_app;
