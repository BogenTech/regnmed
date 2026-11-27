-- Trekkpålegg registered by hand (docs/trekkpalegg.md §6, #339).
--
-- Four things, all insert-only:
--
-- 1. `garnishment_letter`: the letter an order came in, stored like every
--    other document (docs/blob-store.md): content-addressed, hash-checked
--    on every read, bytes in the row or in the object store. Readable only
--    with LONN_LES; never attached to the payroll voucher, which anyone
--    reading the ledger can open.
-- 2. `garnishment` + `garnishment_version`: an order and its versions. The
--    employee and the type are fixed on the order; everything else is per
--    version, and a change or a stop is a NEW version, never an edit. The
--    newest version that may be withheld governs (see regnmed-db
--    `garnishment`).
-- 3. `payroll_garnishment_setting`: the account the trekk is credited to
--    and the company's own payment details for utleggstrekk skatt. Dated,
--    newest applies, like mva_terminordning.
-- 4. `payroll_garnishment`: what a run withheld, per employee and order —
--    what the a-melding and the payslip read back.

create table garnishment_letter (
    id           uuid primary key,
    company_id   uuid not null references company (id),
    filename     text not null check (filename <> ''),
    content_type text not null check (content_type = 'application/pdf' or content_type like 'image/%'),
    byte_size    integer not null check (byte_size > 0),
    sha256       bytea not null check (length(sha256) = 32),
    content      bytea,
    uploaded_by  text not null check (uploaded_by <> ''),
    created_at   timestamptz not null default now(),
    check (content is null or sha256 = sha256(content))
);
create index garnishment_letter_company_idx on garnishment_letter (company_id);
create trigger garnishment_letter_no_delete
    before delete on garnishment_letter
    for each row execute function forbid_ledger_mutation();
create trigger garnishment_letter_offload_only
    before update on garnishment_letter
    for each row execute function guard_content_offload('content', 'sha256');
create trigger garnishment_letter_no_truncate
    before truncate on garnishment_letter
    for each statement execute function forbid_ledger_mutation();
grant select, insert on garnishment_letter to regnmed_app;

create table garnishment (
    id          uuid primary key,
    company_id  uuid not null references company (id),
    employee_id uuid not null references employee (id),
    -- Skatteetaten's three types (§6): samordnet (new scheme), skatt and
    -- ovrige (old scheme; ovrige is never reported in the a-melding).
    kind        text not null check (kind in ('samordnet', 'skatt', 'ovrige')),
    created_by  text not null check (created_by <> ''),
    created_at  timestamptz not null default now()
);
create index garnishment_company_idx on garnishment (company_id);

create table garnishment_version (
    id                 uuid primary key,
    garnishment_id     uuid not null references garnishment (id),
    version            integer not null check (version >= 1),
    -- A monthly amount or a percentage, never both.
    amount_ore         bigint check (amount_ore > 0),
    percent_bp         integer check (percent_bp between 1 and 10000),
    valid_from         date not null,
    valid_to           date,
    kid                text check (kid ~ '^[0-9]{2,25}$'),
    payee_orgnumber    text check (payee_orgnumber ~ '^[0-9]{9}$'),
    payee_account      text check (payee_account ~ '^[0-9]{11}$'),
    -- Ovrige only: the letter's deadline, in days after the pay date.
    due_days           integer check (due_days between 0 and 365),
    letter_id          uuid not null references garnishment_letter (id),
    -- A stop is a version: from it on, nothing is withheld.
    stopped            boolean not null default false,
    -- Decided when the version is written: an active attestation policy
    -- at that moment means another person must approve it before it is
    -- withheld. A policy switched on later does not stop running orders.
    needs_attestation  boolean not null,
    note               text,
    created_by_person  uuid not null references person (id),
    created_by         text not null check (created_by <> ''),
    created_at         timestamptz not null default now(),
    unique (garnishment_id, version),
    check ((amount_ore is null) <> (percent_bp is null)),
    check (valid_to is null or valid_from <= valid_to)
);
create trigger garnishment_append_only
    before update or delete on garnishment
    for each row execute function forbid_ledger_mutation();
create trigger garnishment_no_truncate
    before truncate on garnishment
    for each statement execute function forbid_ledger_mutation();
create trigger garnishment_version_append_only
    before update or delete on garnishment_version
    for each row execute function forbid_ledger_mutation();
create trigger garnishment_version_no_truncate
    before truncate on garnishment_version
    for each statement execute function forbid_ledger_mutation();
grant select, insert on garnishment, garnishment_version to regnmed_app;

-- A registration is attested like an inbox document (docs/attestering.md):
-- the target is one VERSION, so a change is attested on its own.
alter table attestation drop constraint attestation_target_kind_check;
alter table attestation add constraint attestation_target_kind_check
    check (target_kind in ('inbox_document', 'garnishment_version'));

create table payroll_garnishment_setting (
    id              uuid primary key,
    company_id      uuid not null references company (id),
    valid_from      date not null,
    -- The supplier-subledger account the trekk is credited to (§2 step 5).
    account_number  text,
    -- Utleggstrekk skatt is paid as ONE amount per pay date with the
    -- company's own KID, from Skatteetaten's payment details.
    tax_kid         text check (tax_kid ~ '^[0-9]{2,25}$'),
    tax_account     text check (tax_account ~ '^[0-9]{11}$'),
    tax_payee_orgnumber text check (tax_payee_orgnumber ~ '^[0-9]{9}$'),
    set_by          text not null check (set_by <> ''),
    created_at      timestamptz not null default now()
);
create index payroll_garnishment_setting_idx
    on payroll_garnishment_setting (company_id, valid_from desc, created_at desc);
create trigger payroll_garnishment_setting_append_only
    before update or delete on payroll_garnishment_setting
    for each row execute function forbid_ledger_mutation();
create trigger payroll_garnishment_setting_no_truncate
    before truncate on payroll_garnishment_setting
    for each statement execute function forbid_ledger_mutation();
grant select, insert on payroll_garnishment_setting to regnmed_app;

create table payroll_garnishment (
    id              uuid primary key,
    run_id          uuid not null references payroll_run (id),
    employee_id     uuid not null references employee (id),
    garnishment_id  uuid not null references garnishment (id),
    version         integer not null,
    kind            text not null check (kind in ('samordnet', 'skatt', 'ovrige')),
    -- What the order asked for and what the run could take (§3: as much
    -- as possible, never more). Whole kroner, both.
    wanted_ore      bigint not null check (wanted_ore >= 0 and wanted_ore % 100 = 0),
    withheld_ore    bigint not null check (withheld_ore between 0 and wanted_ore
                                           and withheld_ore % 100 = 0),
    kid             text,
    foreign key (garnishment_id, version)
        references garnishment_version (garnishment_id, version)
);
create index payroll_garnishment_run_idx on payroll_garnishment (run_id);
create trigger payroll_garnishment_append_only
    before update or delete on payroll_garnishment
    for each row execute function forbid_ledger_mutation();
create trigger payroll_garnishment_no_truncate
    before truncate on payroll_garnishment
    for each statement execute function forbid_ledger_mutation();
grant select, insert on payroll_garnishment to regnmed_app;
