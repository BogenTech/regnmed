-- Master data: companies, journals, accounts, VAT codes.
--
-- These tables are registries, not ledger history — names and flags may be
-- edited. The ledger itself (voucher/entry, migration 0002) is append-only.
--
-- NOTE: migration files are part of the tamper-evidence story. Treat this
-- directory as append-only in git; sqlx checksums applied migrations and
-- refuses to run if an already-applied file has changed.

create table company (
    id         uuid primary key,
    orgnr      text not null unique check (orgnr ~ '^[0-9]{9}$'),
    name       text not null check (name <> ''),
    created_at timestamptz not null default now()
);

create table journal (
    id         uuid primary key,
    company_id uuid not null references company (id),
    code       text not null check (code <> ''),
    name       text not null,
    created_at timestamptz not null default now(),
    unique (company_id, code)
);

-- Norwegian SAF-T standard VAT codes (representative subset; the complete
-- list ships with the SAF-T module). The rate here is informational — the
-- rate applied to a posting is decided at posting time, since rates change
-- over the years while codes stay stable.
create table vat_code (
    code         text primary key check (code <> ''),
    description  text not null,
    rate_percent numeric(5, 2) not null
);

insert into vat_code (code, description, rate_percent) values
    ('0',  'Ingen merverdiavgiftsbehandling (anskaffelser)', 0),
    ('1',  'Fradragsberettiget innenlands inngående mva, alminnelig sats', 25),
    ('11', 'Fradragsberettiget innenlands inngående mva, redusert sats, næringsmidler', 15),
    ('13', 'Fradragsberettiget innenlands inngående mva, redusert sats', 12),
    ('3',  'Utgående mva, alminnelig sats', 25),
    ('31', 'Utgående mva, redusert sats, næringsmidler', 15),
    ('33', 'Utgående mva, redusert sats', 12),
    ('5',  'Mva-fritt salg, nullsats', 0),
    ('52', 'Utførsel av varer og tjenester, nullsats', 0),
    ('6',  'Omsetning utenfor merverdiavgiftsloven', 0),
    ('7',  'Ingen mva-behandling (inntekter)', 0);

create table account (
    id         uuid primary key,
    company_id uuid not null references company (id),
    -- Four-digit NS 4102 account numbers, e.g. 1920 bank, 3000 salgsinntekt.
    number     text not null check (number ~ '^[0-9]{4}$'),
    name       text not null check (name <> ''),
    vat_code   text references vat_code (code),
    active     boolean not null default true,
    created_at timestamptz not null default now(),
    unique (company_id, number)
);
