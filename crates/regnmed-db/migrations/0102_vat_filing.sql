-- The mva-melding filed by a person through ID-porten (docs/mva.md).
--
-- What is sent is stored before it is sent, and the record never changes:
-- status is computed from the insert-only events.
create table vat_filing (
    id                  uuid primary key,
    company_id          uuid not null references company (id),
    year                int  not null,
    period              int  not null check (period between 1 and 6),
    melding_xml         text not null check (melding_xml <> ''),
    melding_sha256      bytea not null check (octet_length(melding_sha256) = 32),
    assessed_kr         bigint not null,
    requested_by_person uuid not null references person (id),
    requested_by        text not null check (requested_by <> ''),
    created_at          timestamptz not null default now()
);
create index vat_filing_period on vat_filing (company_id, year, period, created_at desc);

create table vat_filing_event (
    id         uuid primary key,
    filing_id  uuid not null references vat_filing (id),
    kind       text not null check (kind in (
        'opprettet', 'godkjent', 'fornyet', 'konvolutt', 'bekrefter', 'bekreftelse_avvist',
        'avvik',
        'oppretter_instans', 'instans_opprettet', 'sendt', 'kvittert', 'avvist',
        'uavklart', 'feilet', 'ma_godkjennes', 'avbrutt', 'lukket_etter_kontroll')),
    detail     jsonb not null default '{}',
    created_at timestamptz not null default now(),
    -- A person's close always says what they found in Altinn.
    check (kind <> 'lukket_etter_kontroll' or detail->>'funn' in ('levert', 'ikke_levert'))
);
create index vat_filing_event_filing on vat_filing_event (filing_id, id);
-- Skatteetaten answers a filing once, however often the answer is fetched.
create unique index vat_filing_event_one_outcome on vat_filing_event (filing_id)
    where kind in ('kvittert', 'avvist');

-- Skatteetaten's files for a filing, kept as received.
create table vat_filing_document (
    filing_id    uuid not null references vat_filing (id),
    data_type    text not null check (data_type in (
        'betalingsinformasjon', 'kvittering', 'valideringsresultat')),
    content_type text not null,
    content      bytea not null,
    created_at   timestamptz not null default now(),
    primary key (filing_id, data_type)
);

-- Which ID-porten identity a regnmed person files as. Pairwise per client,
-- so never the national ID. Insert-only and versioned: the newest row per
-- (person, issuer, client) applies. A first sign-in binds it, and only an
-- operator can release it, with a reason (`regnmed idporten-rebind`).
create table idporten_identity (
    id          uuid primary key,
    person_id   uuid not null references person (id),
    issuer      text not null check (issuer <> ''),
    client_id   text not null check (client_id <> ''),
    -- Null: released by an operator, so the next sign-in binds again.
    sub         text check (sub <> ''),
    source      text not null check (source in ('godkjenning', 'operator')),
    reason      text check (reason <> ''),
    recorded_by text not null check (recorded_by <> ''),
    created_at  timestamptz not null default clock_timestamp(),
    check ((source = 'operator') = (sub is null)),
    check ((source = 'operator') = (reason is not null))
);
create index idporten_identity_current
    on idporten_identity (person_id, issuer, client_id, created_at desc, id desc);

do $$
declare t text;
begin
    foreach t in array array['vat_filing', 'vat_filing_event', 'vat_filing_document',
                            'idporten_identity'] loop
        execute format('create trigger %I before update or delete on %I
                        for each row execute function forbid_ledger_mutation()',
                       t || '_append_only', t);
        execute format('create trigger %I before truncate on %I
                        for each statement execute function forbid_ledger_mutation()',
                       t || '_no_truncate', t);
    end loop;
end $$;

-- The one short-lived row: the pending login, then the person's sealed
-- token. It is deleted when used or expired, and only ever holds ciphertext.
create table vat_filing_grant (
    filing_id    uuid primary key references vat_filing (id),
    person_id    uuid not null references person (id),
    kind         text not null check (kind in ('innlogging', 'idporten', 'altinn')),
    state_sha256 bytea check (octet_length(state_sha256) = 32),
    sealed       bytea not null,
    expires_at   timestamptz not null,
    check ((kind = 'innlogging') = (state_sha256 is not null))
);
create index vat_filing_grant_expiry on vat_filing_grant (expires_at);

alter table job drop constraint job_kind_check;
alter table job add constraint job_kind_check check (kind in (
    'saft_import', 'audit_report', 'bank_sync',
    'peppol_lookup', 'invoice_dispatch', 'peppol_sender_setup',
    'vat_filing_submit'));

grant select, insert on vat_filing, vat_filing_event, vat_filing_document,
    idporten_identity to regnmed_app;
grant select, insert, update, delete on vat_filing_grant to regnmed_app;
