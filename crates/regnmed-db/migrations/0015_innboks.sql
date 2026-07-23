-- Bilagsinnboks: the daily loop between client and regnskapsfører.
-- Clients upload dokumentasjon (receipts, invoices) as it happens; the
-- accountant turns each document into a posted voucher — or rejects it
-- with a note.
--
-- Honesty rules, enforced here:
-- - Document content is immutable from the moment it arrives (SHA-256
--   stored at upload, column grants + trigger forbid content changes).
-- - A decision is one-way: 'ny' → 'bokfort' (with the voucher id) or
--   'ny' → 'avvist' (with a note); re-deciding is rejected by trigger.
-- - Nothing is ever deleted — a rejected document with its note is part
--   of the story of the bookkeeping, like everything else here.

create table inbox_document (
    id           uuid primary key,
    company_id   uuid not null references company (id),
    filename     text not null check (filename <> ''),
    content_type text not null,
    byte_size    bigint not null check (byte_size > 0),
    sha256       bytea not null check (octet_length(sha256) = 32),
    content      bytea not null,
    uploaded_by  text not null check (uploaded_by <> ''),
    created_at   timestamptz not null default now(),
    status       text not null default 'ny' check (status in ('ny', 'bokfort', 'avvist')),
    voucher_id   uuid references voucher (id),
    decided_by   text,
    decided_at   timestamptz,
    note         text,
    check (status <> 'bokfort' or voucher_id is not null),
    check (status = 'ny' or (decided_by is not null and decided_at is not null))
);

create index inbox_document_company_idx on inbox_document (company_id, status, created_at desc);

create function guard_inbox_update() returns trigger
language plpgsql as $$
begin
    if old.status <> 'ny' then
        raise exception 'inbox document % is already decided (%)', old.id, old.status;
    end if;
    if new.content is distinct from old.content
       or new.sha256 is distinct from old.sha256
       or new.filename is distinct from old.filename
       or new.content_type is distinct from old.content_type
       or new.byte_size is distinct from old.byte_size
       or new.company_id is distinct from old.company_id
       or new.uploaded_by is distinct from old.uploaded_by
       or new.created_at is distinct from old.created_at then
        raise exception 'inbox document content is immutable';
    end if;
    return new;
end;
$$;

create trigger inbox_update_guard
    before update on inbox_document
    for each row execute function guard_inbox_update();
create trigger inbox_no_delete
    before delete on inbox_document
    for each row execute function forbid_ledger_mutation();
create trigger inbox_no_truncate
    before truncate on inbox_document
    for each statement execute function forbid_ledger_mutation();

grant select, insert on inbox_document to regnmed_app;
grant update (status, voucher_id, decided_by, decided_at, note) on inbox_document to regnmed_app;
