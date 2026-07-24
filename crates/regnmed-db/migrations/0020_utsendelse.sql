-- E-postutsendelse av salgsdokumenter (docs/faktura.md, #32).
--
-- Sending is always an explicit human action; every send is one row in
-- an insert-only log — who sent what to whom, when. The utsendelse id
-- doubles as the mail's queue id (Nats-Msg-Id), so a retried publish
-- deduplicates in the stream and the log row IS the delivery evidence
-- regnmed holds. Replies go to the company's own address, never to the
-- platform.

alter table company add column email text;
grant update (email) on company to regnmed_app;

create table utsendelse (
    id          uuid primary key,
    company_id  uuid not null references company (id),
    invoice_id  uuid references invoice (id),
    reminder_id uuid references invoice_reminder (id),
    to_email    text not null check (to_email <> ''),
    subject     text not null,
    sent_by     text not null check (sent_by <> ''),
    created_at  timestamptz not null default now(),
    check (invoice_id is not null or reminder_id is not null)
);

create index utsendelse_invoice on utsendelse (invoice_id);

create trigger utsendelse_append_only
    before update or delete on utsendelse
    for each row execute function forbid_ledger_mutation();
create trigger utsendelse_no_truncate
    before truncate on utsendelse
    for each statement execute function forbid_ledger_mutation();

grant select, insert on utsendelse to regnmed_app;
