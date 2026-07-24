-- Timeføring (docs/timer.md, #38). Hours are the inventory of
-- tjenesteytende SMB-er.
--
-- Minutes are integers — no floats, same discipline as øre. Entries
-- are working data (editable, deletable) until either (a) the month is
-- LOCKED — insert-only lock history exactly like period_lock, because
-- locked hours feed lønn and faktura and are then evidence — or
-- (b) the entry is FAKTURERT (one-way link to the invoice). Both are
-- enforced by trigger, independently of the application.

create table time_entry (
    id           uuid primary key,
    company_id   uuid not null references company (id),
    person_id    uuid not null references person (id),
    dato         date not null,
    minutter     integer not null check (minutter > 0 and minutter <= 1440),
    beskrivelse  text not null check (beskrivelse <> ''),
    -- Prosjekt from the dimension registry (docs/dimensjoner.md).
    prosjekt_id  uuid references dimension (id),
    fakturerbar  boolean not null default false,
    timesats_ore bigint check (timesats_ore is null or timesats_ore >= 0),
    -- Set once, when the hours are billed; entry becomes immutable.
    invoice_id   uuid references invoice (id),
    created_by   text not null check (created_by <> ''),
    created_at   timestamptz not null default now(),
    updated_at   timestamptz not null default now(),
    check (not fakturerbar or timesats_ore is not null)
);

create index time_entry_company_date on time_entry (company_id, dato);
create index time_entry_unbilled
    on time_entry (company_id) where fakturerbar and invoice_id is null;

create table timesheet_lock (
    id             uuid primary key,
    company_id     uuid not null references company (id),
    locked_through date not null,
    locked_by      text not null check (locked_by <> ''),
    note           text,
    created_at     timestamptz not null default now()
);

create index timesheet_lock_company_idx on timesheet_lock (company_id, created_at desc);

create trigger timesheet_lock_append_only
    before update or delete on timesheet_lock
    for each row execute function forbid_ledger_mutation();
create trigger timesheet_lock_no_truncate
    before truncate on timesheet_lock
    for each statement execute function forbid_ledger_mutation();

create function current_timesheet_lock(cid uuid) returns date
language sql stable as $$
    select locked_through from timesheet_lock
    where company_id = cid
    order by created_at desc, id desc
    limit 1
$$;

create function timesheet_entry_guard() returns trigger
language plpgsql as $$
declare
    lock_date date;
begin
    -- Billing hours from a locked month is legitimate (lock for lønn,
    -- then fakturer): the ONLY change allowed on a locked entry is the
    -- one-way invoice marker, everything else identical.
    if tg_op = 'UPDATE'
       and old.invoice_id is null and new.invoice_id is not null
       and new.company_id = old.company_id and new.person_id = old.person_id
       and new.dato = old.dato and new.minutter = old.minutter
       and new.beskrivelse = old.beskrivelse
       and new.prosjekt_id is not distinct from old.prosjekt_id
       and new.fakturerbar = old.fakturerbar
       and new.timesats_ore is not distinct from old.timesats_ore then
        return new;
    end if;
    if tg_op in ('UPDATE', 'DELETE') then
        if old.invoice_id is not null then
            raise exception 'time entry is fakturert and immutable';
        end if;
        lock_date := current_timesheet_lock(old.company_id);
        if lock_date is not null and old.dato <= lock_date then
            raise exception 'timesheet is locked through %', lock_date;
        end if;
    end if;
    if tg_op in ('INSERT', 'UPDATE') then
        lock_date := current_timesheet_lock(new.company_id);
        if lock_date is not null and new.dato <= lock_date then
            raise exception 'timesheet is locked through %', lock_date;
        end if;
        return new;
    end if;
    return old;
end;
$$;

create trigger time_entry_guard
    before insert or update or delete on time_entry
    for each row execute function timesheet_entry_guard();

grant select, insert, update, delete on time_entry to regnmed_app;
grant select, insert on timesheet_lock to regnmed_app;
