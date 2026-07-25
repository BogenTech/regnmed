-- E-post-inn til bilagsinnboksen (docs/epost-inn.md, #35).
--
-- Leverandører og ansatte sender kvitteringer på e-post. Meldingen
-- kommer inn på plattformens ENE mail-rail (regnids arbeidere, NATS —
-- ingen andre SMTP-stakk her), og vedleggene blir innboksdokumenter
-- gjennom nøyaktig samme uforanderlige vei som en opplasting
-- (migration 0015).
--
-- Tre ærlighetskrav styrer utformingen:
--
-- 1. Adressen er en KAPABILITET: uforutsigbar, per selskap, og kan
--    roteres. Den gir bare rett til å LEVERE noe i innboksen — aldri
--    til å lese eller bestemme noe.
-- 2. Ukjent avsender havner i KARANTENE. Ikke stille importert (da
--    kunne hvem som helst fylle innboksen), og ikke stille forkastet
--    (da forsvinner et bilag noen faktisk sendte). En admin ser det og
--    avgjør.
-- 3. Hver mottatt melding er en rad som blir stående, også de avviste.
--    Rå melding og brødtekst lagres som dokumentasjon av opprinnelse.

create table company_mail_inbox (
    id          uuid primary key,
    company_id  uuid not null references company (id),
    -- Lokaldelen av mottaksadressen: bilag-<navn>-<tilfeldig>.
    local_part  text not null unique check (local_part ~ '^[a-z0-9][a-z0-9-]{4,62}$'),
    active      boolean not null default true,
    created_by  text not null check (created_by <> ''),
    created_at  timestamptz not null default now(),
    revoked_at  timestamptz
);

create index company_mail_inbox_company_idx on company_mail_inbox (company_id, active);

create function company_mail_inbox_guard() returns trigger
language plpgsql as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'mottaksadresser slettes ikke — de deaktiveres';
    end if;
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.local_part is distinct from old.local_part
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'mottaksadressens innhold er uforanderlig';
    end if;
    if old.active = false and new.active = true then
        raise exception 'en tilbakekalt adresse kan ikke gjenoppstå — lag en ny';
    end if;
    return new;
end;
$$;

create trigger company_mail_inbox_change_guard
    before update or delete on company_mail_inbox
    for each row execute function company_mail_inbox_guard();
create trigger company_mail_inbox_no_truncate
    before truncate on company_mail_inbox
    for each statement execute function forbid_ledger_mutation();

grant select, insert on company_mail_inbox to regnmed_app;
grant update (active, revoked_at) on company_mail_inbox to regnmed_app;

-- Avsenderlisten: e-postadresse eller domene (@grossisten.no) som
-- slipper rett inn. Admin styrer den; historikken beholdes.
create table mail_sender_allow (
    id         uuid primary key,
    company_id uuid not null references company (id),
    -- Lowercased: "post@grossisten.no" eller "@grossisten.no".
    sender     text not null check (sender <> '' and sender = lower(sender)),
    note       text,
    active     boolean not null default true,
    created_by text not null check (created_by <> ''),
    created_at timestamptz not null default now()
);

create unique index mail_sender_allow_unique
    on mail_sender_allow (company_id, sender) where active;

create trigger mail_sender_allow_no_truncate
    before truncate on mail_sender_allow
    for each statement execute function forbid_ledger_mutation();

grant select, insert, delete on mail_sender_allow to regnmed_app;
grant update (active, note) on mail_sender_allow to regnmed_app;

create table inbox_mail (
    id            uuid primary key,
    company_id    uuid not null references company (id),
    -- Avsenderens Message-Id: samme melding levert to ganger blir ETT
    -- innslag (leverandører og køer gjentar seg).
    message_id    text not null,
    from_address  text not null check (from_address <> ''),
    subject       text,
    -- Brødteksten, lagret som dokumentasjon av opprinnelse.
    body          text,
    -- Hele den mottatte meldingen (JSON m/ base64-vedlegg), så et
    -- karantenesatt bilag kan slippes gjennom senere uten at avsender
    -- må sende på nytt.
    raw           bytea not null,
    antall_vedlegg integer not null default 0 check (antall_vedlegg >= 0),
    received_at   timestamptz not null default now(),
    status        text not null check (status in ('mottatt', 'karantene', 'avvist')),
    note          text,
    decided_by    text,
    decided_at    timestamptz,
    check (status <> 'avvist' or note is not null),
    unique (company_id, message_id)
);

create index inbox_mail_company_idx on inbox_mail (company_id, received_at desc);

create function inbox_mail_guard() returns trigger
language plpgsql as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'mottatt e-post slettes ikke';
    end if;
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.message_id is distinct from old.message_id
       or new.from_address is distinct from old.from_address
       or new.subject is distinct from old.subject
       or new.body is distinct from old.body
       or new.raw is distinct from old.raw
       or new.received_at is distinct from old.received_at then
        raise exception 'mottatt e-post er uforanderlig';
    end if;
    -- Karantene er den eneste tilstanden som kan avgjøres, og bare én gang.
    if old.status <> 'karantene' then
        raise exception 'e-posten er allerede %', old.status;
    end if;
    if new.status not in ('mottatt', 'avvist') then
        raise exception 'ugyldig utfall %', new.status;
    end if;
    return new;
end;
$$;

create trigger inbox_mail_change_guard
    before update or delete on inbox_mail
    for each row execute function inbox_mail_guard();
create trigger inbox_mail_no_truncate
    before truncate on inbox_mail
    for each statement execute function forbid_ledger_mutation();

grant select, insert on inbox_mail to regnmed_app;
grant update (status, note, decided_by, decided_at) on inbox_mail to regnmed_app;

-- Vedleggene slik de kom, dekodet én gang ved mottak. En e-post i
-- karantene kan dermed slippes gjennom uten at avsenderen må sende på
-- nytt, og uten at vi tolker den lagrede meldingen om igjen.
create table inbox_mail_attachment (
    id           uuid primary key,
    mail_id      uuid not null references inbox_mail (id),
    filename     text not null check (filename <> ''),
    content_type text not null,
    byte_size    bigint not null check (byte_size > 0),
    sha256       bytea not null check (octet_length(sha256) = 32),
    content      bytea not null
);

create index inbox_mail_attachment_mail_idx on inbox_mail_attachment (mail_id);

create trigger inbox_mail_attachment_append_only
    before update or delete on inbox_mail_attachment
    for each row execute function forbid_ledger_mutation();
create trigger inbox_mail_attachment_no_truncate
    before truncate on inbox_mail_attachment
    for each statement execute function forbid_ledger_mutation();

grant select, insert on inbox_mail_attachment to regnmed_app;

-- Opprinnelsen til et dokument som kom med e-post.
alter table inbox_document add column inbox_mail_id uuid references inbox_mail (id);

-- Opprinnelsen er en del av dokumentets uforanderlige innhold. Kolonnen
-- ligger allerede utenfor update-grantet fra 0015; vakten sier det også,
-- slik doktrinen krever (grants OG trigger).
create or replace function guard_inbox_update() returns trigger
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
       or new.created_at is distinct from old.created_at
       or new.inbox_mail_id is distinct from old.inbox_mail_id then
        raise exception 'inbox document content is immutable';
    end if;
    return new;
end;
$$;
