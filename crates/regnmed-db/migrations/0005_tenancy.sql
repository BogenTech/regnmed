-- Marketplace tenancy: people, firms (regnskapskontor / revisjonsforetak)
-- and engagements (oppdrag) connecting firms to client companies.
--
-- Authorization model: the OIDC token proves identity only (person.oidc_sub).
-- What a person may do is resolved from these tables —
--   person → company_member                       (direct: owner, staff)
--   person → firm_member → engagement → company   (accountant / auditor)

create table person (
    id         uuid primary key,
    -- Subject claim from the identity provider. Identity only — never
    -- roles: an accountant with 60 clients cannot carry that in a token.
    oidc_sub   text not null unique check (oidc_sub <> ''),
    name       text,
    email      text,
    created_at timestamptz not null default now()
);

create table firm (
    id         uuid primary key,
    orgnr      text not null unique check (orgnr ~ '^[0-9]{9}$'),
    name       text not null check (name <> ''),
    -- 'regnskap' (regnskapsførerforetak) or 'revisjon' (revisjonsforetak).
    -- Deliberately one kind per firm: independence rules mean the same firm
    -- does not both keep and audit the same books.
    kind       text not null check (kind in ('regnskap', 'revisjon')),
    -- Set when autorisasjon is verified against Finanstilsynet's register;
    -- the marketplace only lists verified firms.
    autorisasjon_verified_at timestamptz,
    created_at timestamptz not null default now()
);

create table firm_member (
    firm_id    uuid not null references firm (id),
    person_id  uuid not null references person (id),
    role       text not null check (role in ('admin', 'ansatt')),
    active     boolean not null default true,
    created_at timestamptz not null default now(),
    primary key (firm_id, person_id)
);

create table company_member (
    company_id uuid not null references company (id),
    person_id  uuid not null references person (id),
    -- Access levels: admin ⊃ bokforing ⊃ les.
    role       text not null check (role in ('admin', 'bokforing', 'les')),
    active     boolean not null default true,
    created_at timestamptz not null default now(),
    primary key (company_id, person_id)
);

-- An oppdrag: the contractual relationship (oppdragsavtale, jf.
-- regnskapsførerloven/GRFS) that gives a firm's staff access to a client
-- company. 'regnskap' grants bookkeeping access; 'revisjon' grants read
-- access including independent chain verification. Ended engagements keep
-- their row (valid_to set) — they are the history of who had access, so
-- the application role gets no DELETE and may only update valid_to.
create table engagement (
    id         uuid primary key,
    firm_id    uuid not null references firm (id),
    company_id uuid not null references company (id),
    kind       text not null check (kind in ('regnskap', 'revisjon')),
    valid_from date not null default current_date,
    valid_to   date check (valid_to >= valid_from),
    created_at timestamptz not null default now()
);

-- At most one open engagement per firm/company/kind.
create unique index engagement_active_uq
    on engagement (firm_id, company_id, kind)
    where valid_to is null;

create index engagement_company_idx on engagement (company_id);
create index firm_member_person_idx on firm_member (person_id);
create index company_member_person_idx on company_member (person_id);

-- Application role: read everything, append memberships/engagements,
-- update only what may legitimately change. No DELETE anywhere.
grant select, insert on person, firm, firm_member, company_member, engagement to regnmed_app;
grant update (name, email) on person to regnmed_app;
grant update (name, autorisasjon_verified_at) on firm to regnmed_app;
grant update (role, active) on firm_member to regnmed_app;
grant update (role, active) on company_member to regnmed_app;
grant update (valid_to) on engagement to regnmed_app;
