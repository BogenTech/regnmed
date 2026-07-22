-- Periodelåsing (ajourhold) and bilagsvedlegg (oppbevaringsplikt).
--
-- Period locks: an insert-only history of "locked through" dates per
-- company; the current lock is the latest row, so every advance and
-- every reopening is audit trail. The database re-checks the lock on
-- voucher insert (layer 2), independent of the application check.
--
-- Attachments are dokumentasjon (bokføringsloven §13): append-only like
-- the ledger (same trigger function), content SHA-256 stored for
-- re-verification. Attachments may be added after posting — completing
-- documentation is allowed, changing or removing it is not. Chain-level
-- anchoring of attachment sets rides with external anchoring (M6).

create table period_lock (
    id             uuid primary key,
    company_id     uuid not null references company (id),
    locked_through date not null,
    set_by         text not null check (set_by <> ''),
    created_at     timestamptz not null default now()
);

create index period_lock_company_idx on period_lock (company_id, created_at desc);

create function current_period_lock(cid uuid) returns date
language sql stable as $$
    select locked_through from period_lock
    where company_id = cid
    order by created_at desc, id desc
    limit 1
$$;

create function forbid_locked_period_posting() returns trigger
language plpgsql as $$
declare
    lock_date date;
begin
    lock_date := current_period_lock(new.company_id);
    if lock_date is not null and new.voucher_date <= lock_date then
        raise exception
            'period is locked through %: voucher dated % cannot be posted — correct in an open period',
            lock_date, new.voucher_date;
    end if;
    return new;
end;
$$;

create trigger voucher_period_lock
    before insert on voucher
    for each row execute function forbid_locked_period_posting();

create table attachment (
    id           uuid primary key,
    company_id   uuid not null references company (id),
    voucher_id   uuid not null references voucher (id),
    filename     text not null check (filename <> ''),
    content_type text not null,
    byte_size    bigint not null check (byte_size > 0),
    sha256       bytea not null check (octet_length(sha256) = 32),
    content      bytea not null,
    uploaded_by  text not null check (uploaded_by <> ''),
    created_at   timestamptz not null default now()
);

create index attachment_voucher_idx on attachment (voucher_id);

create trigger attachment_append_only
    before update or delete on attachment
    for each row execute function forbid_ledger_mutation();

create trigger attachment_no_truncate
    before truncate on attachment
    for each statement execute function forbid_ledger_mutation();

grant select, insert on period_lock, attachment to regnmed_app;
