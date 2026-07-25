-- Betalingsliste og remittering (docs/betaling.md, #33).
--
-- A betalingskjøring is evidence with a one-way lifecycle, enforced by
-- trigger like the other decision flows:
--   utkast → godkjent   (the pain.001 file is rendered, stored and
--                        hashed — creating the list and approving it
--                        for export are SEPARATE audited actions)
--   godkjent → utbetalt (the utbetalingsbilag is posted and every item
--                        reskontro-matched, in one transaction)
--   utkast → annullert  (a list that never should have existed stays
--                        visible with who cancelled it)
-- Items snapshot the creditor data (name, kontonummer, KID) at
-- creation — the file must be reproducible from the rows forever,
-- whatever the party register says later.

-- Leverandørens kontonummer: editable contact data like address/email
-- (0019 pattern) — validated MOD11 in code before it is stored.
alter table party add column bank_account text;
grant update (bank_account) on party to regnmed_app;

create table payment_run (
    id                 uuid primary key,
    company_id         uuid not null references company (id),
    status             text not null default 'utkast'
        check (status in ('utkast', 'godkjent', 'utbetalt', 'annullert')),
    -- The 11-digit kontonummer the payments debit (pain.001 DbtrAcct).
    debitor_konto      text not null check (debitor_konto ~ '^[0-9]{11}$'),
    execution_date     date not null,
    created_by         text not null check (created_by <> ''),
    created_at         timestamptz not null default now(),
    approved_by        text,
    approved_at        timestamptz,
    file               bytea,
    file_sha256        bytea check (file_sha256 is null or octet_length(file_sha256) = 32),
    settled_voucher_id uuid references voucher (id),
    settled_at         timestamptz,
    annullert_by       text,
    annullert_at       timestamptz,
    check (status in ('utkast', 'annullert') or (approved_by is not null and file is not null
                                                 and file_sha256 is not null)),
    check (status <> 'utbetalt' or settled_voucher_id is not null),
    check (status <> 'annullert' or annullert_by is not null)
);

create index payment_run_company_idx on payment_run (company_id, created_at desc);

create function payment_run_guard() returns trigger
language plpgsql as $$
begin
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.debitor_konto is distinct from old.debitor_konto
       or new.execution_date is distinct from old.execution_date
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'payment run content is immutable';
    end if;
    if old.status = 'utkast' and new.status in ('godkjent', 'annullert') then
        return new;
    end if;
    if old.status = 'godkjent' and new.status = 'utbetalt'
       and new.approved_by = old.approved_by and new.approved_at = old.approved_at
       and new.file = old.file and new.file_sha256 = old.file_sha256 then
        return new;
    end if;
    raise exception 'payment run % kan ikke gå fra % til %', old.id, old.status, new.status;
end;
$$;

create trigger payment_run_update_guard
    before update on payment_run
    for each row execute function payment_run_guard();
create trigger payment_run_no_delete
    before delete on payment_run
    for each row execute function forbid_ledger_mutation();
create trigger payment_run_no_truncate
    before truncate on payment_run
    for each statement execute function forbid_ledger_mutation();

grant select, insert on payment_run to regnmed_app;
grant update (status, approved_by, approved_at, file, file_sha256,
              settled_voucher_id, settled_at, annullert_by, annullert_at)
    on payment_run to regnmed_app;

create table payment_run_item (
    id             uuid primary key,
    run_id         uuid not null references payment_run (id),
    -- The open leverandør post this payment settles.
    entry_id       uuid not null references entry (id),
    belop_ore      bigint not null check (belop_ore > 0),
    kreditor_navn  text not null check (kreditor_navn <> ''),
    kreditor_konto text not null check (kreditor_konto ~ '^[0-9]{11}$'),
    kid            text,
    melding        text,
    created_at     timestamptz not null default now()
);

create index payment_run_item_run_idx on payment_run_item (run_id);

create trigger payment_run_item_append_only
    before update or delete on payment_run_item
    for each row execute function forbid_ledger_mutation();
create trigger payment_run_item_no_truncate
    before truncate on payment_run_item
    for each statement execute function forbid_ledger_mutation();

grant select, insert on payment_run_item to regnmed_app;
