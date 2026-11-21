-- E-invoice dispatch over Peppol (docs/efaktura.md §3.3, §3.6, §3.7).
--
-- `utsendelse` stays the one insert-only send log, with a channel. A
-- Peppol row has no e-mail address and always points at the stored XML.
alter table utsendelse
    add column channel text not null default 'epost'
        check (channel in ('epost', 'peppol')),
    add column peppol_receiver text,
    add column attachment_id uuid references attachment (id),
    alter column to_email drop not null;

alter table utsendelse add constraint utsendelse_channel_address check (
    (channel = 'epost' and to_email is not null and peppol_receiver is null)
    or (channel = 'peppol' and peppol_receiver is not null and attachment_id is not null)
);

-- Status is computed from these rows, never stored. event_key is
-- deterministic, so a webhook and the reconciliation that see the same
-- status write one row.
create table dispatch_event (
    id          uuid primary key,
    dispatch_id uuid not null references utsendelse (id),
    event_key   text not null unique check (event_key <> ''),
    attempt     int  not null check (attempt > 0),
    kind        text not null check (kind in (
        'sendeforsok', 'ikke_sendt', 'uavklart', 'akseptert',
        'leveringsfeil', 'avvist', 'levert', 'mlr', 'svar')),
    detail      text not null default '',
    received_at timestamptz not null default now()
);
create index dispatch_event_dispatch on dispatch_event (dispatch_id, attempt);

create trigger dispatch_event_append_only
    before update or delete on dispatch_event
    for each row execute function forbid_ledger_mutation();
create trigger dispatch_event_no_truncate
    before truncate on dispatch_event
    for each statement execute function forbid_ledger_mutation();

-- Which provider document an attempt became.
create table dispatch_provider_document (
    provider_document_id text primary key check (provider_document_id <> ''),
    dispatch_id uuid not null references utsendelse (id),
    attempt     int  not null check (attempt > 0),
    linked_at   timestamptz not null default now(),
    unique (dispatch_id, attempt)
);

create trigger dispatch_provider_document_append_only
    before update or delete on dispatch_provider_document
    for each row execute function forbid_ledger_mutation();
create trigger dispatch_provider_document_no_truncate
    before truncate on dispatch_provider_document
    for each statement execute function forbid_ledger_mutation();

-- A company's sender at the provider. Verification changes over time, so
-- this is operational state and not history.
create table peppol_sender (
    company_id          uuid primary key references company (id),
    provider_company_id text not null unique check (provider_company_id <> ''),
    is_verified         boolean not null default false,
    created_at          timestamptz not null default now(),
    updated_at          timestamptz not null default now()
);

-- Two clicks on Send become one open job.
alter table job add column dedup_key text check (dedup_key is null or dedup_key <> '');
create unique index job_open_dedup on job (company_id, kind, dedup_key)
    where dedup_key is not null and status in ('queued', 'running');

alter table job drop constraint job_kind_check;
alter table job add constraint job_kind_check check (kind in (
    'saft_import', 'audit_report', 'bank_sync',
    'peppol_lookup', 'invoice_dispatch', 'peppol_sender_setup'));

grant select, insert on dispatch_event, dispatch_provider_document to regnmed_app;
grant select, insert on peppol_sender to regnmed_app;
grant update (is_verified, updated_at) on peppol_sender to regnmed_app;
