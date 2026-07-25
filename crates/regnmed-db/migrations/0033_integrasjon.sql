-- Maskin-tilgang til API-et (docs/integrations.md, #45).
--
-- «Nettbutikken skal legge ordrene inn selv» krever en identitet som
-- ikke er et menneske. Modellen er den samme som ellers, og det er
-- poenget:
--
--   TOKENET beviser identitet (client_credentials fra vår IdP, regnid —
--   regnmed utsteder aldri egne API-nøkler), og
--   REGNMED avgjør hva den identiteten får gjøre.
--
-- En integrasjon er derfor en `person` med kind = 'integrasjon': samme
-- prinsipal-modell, samme tilgangsoppslag, samme attribusjon. Da finnes
-- det ingen egen autorisasjonsvei for maskiner som kan utvikle egne
-- hull — created_by på et bilag navngir roboten like presist som det
-- navngir et menneske.

alter table person add column kind text not null default 'menneske'
    check (kind in ('menneske', 'integrasjon'));

-- Et tomt person-skall (en klient som ringte før den var registrert)
-- kan bli en integrasjon; regnmed-appen trenger derfor å sette kind.
grant update (kind) on person to regnmed_app;

-- Metadata om maskinklienten. client_id ER person.oidc_sub: tokenets
-- subject. Ingen hemmelighet lagres her — den bor hos IdP-en.
create table integration (
    id             uuid primary key,
    person_id      uuid not null unique references person (id),
    navn           text not null check (navn <> ''),
    -- Hvem man ringer når integrasjonen oppfører seg rart.
    kontakt        text,
    -- Kall per minutt per integrasjon. Standarden er romslig for et
    -- kassasystem og streng nok til å beskytte minnebudsjettet
    -- (docs/frugality.md).
    rate_limit_min integer not null default 120 check (rate_limit_min between 1 and 10000),
    registrert_av  text not null check (registrert_av <> ''),
    created_at     timestamptz not null default now()
);

create trigger integration_no_delete
    before delete on integration
    for each row execute function forbid_ledger_mutation();
create trigger integration_no_truncate
    before truncate on integration
    for each statement execute function forbid_ledger_mutation();

grant select, insert on integration to regnmed_app;
grant update (navn, kontakt, rate_limit_min) on integration to regnmed_app;

-- Tilgangen, modellert som et oppdrag: en admin GIR integrasjonen
-- tilgang til sitt selskap på et nivå, og kan trekke den tilbake.
-- valid_to er EKSKLUSIV (som engagement, 0013) — tilbakekalling virker
-- i samme øyeblikk, ikke ved midnatt.
create table integration_grant (
    id             uuid primary key,
    integration_id uuid not null references integration (id),
    company_id     uuid not null references company (id),
    access         text not null check (access in ('les', 'bokforing')),
    valid_from     date not null default current_date,
    valid_to       date,
    created_by     text not null check (created_by <> ''),
    created_at     timestamptz not null default now(),
    revoked_by     text,
    check (valid_to is null or valid_to >= valid_from)
);

create index integration_grant_company_idx on integration_grant (company_id, integration_id);
-- Én levende tilgang per integrasjon per selskap; historikken beholdes.
create unique index integration_grant_active
    on integration_grant (integration_id, company_id) where valid_to is null;

create function integration_grant_guard() returns trigger
language plpgsql as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'tilganger slettes ikke — de trekkes tilbake';
    end if;
    if new.id is distinct from old.id
       or new.integration_id is distinct from old.integration_id
       or new.company_id is distinct from old.company_id
       or new.access is distinct from old.access
       or new.valid_from is distinct from old.valid_from
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'tilgangens innhold er uforanderlig — trekk den tilbake og gi en ny';
    end if;
    if old.valid_to is not null then
        raise exception 'tilgangen er allerede trukket tilbake';
    end if;
    return new;
end;
$$;

create trigger integration_grant_change_guard
    before update or delete on integration_grant
    for each row execute function integration_grant_guard();
create trigger integration_grant_no_truncate
    before truncate on integration_grant
    for each statement execute function forbid_ledger_mutation();

grant select, insert on integration_grant to regnmed_app;
grant update (valid_to, revoked_by) on integration_grant to regnmed_app;

-- Aktivitetsloggen, delt i to av hensyn til volum:
--
--   integration_call    hver ENDRENDE forespørsel, med sitt utfall. Det
--                       er disse en admin (og en revisor) vil se.
--   integration_usage   en teller per integrasjon, selskap og dag, som
--                       dekker ALLE kall — også lesingene, som kan være
--                       mange og hver for seg lite interessante.
create table integration_call (
    id             uuid primary key,
    integration_id uuid not null references integration (id),
    company_id     uuid references company (id),
    method         text not null,
    path           text not null,
    status         integer not null,
    created_at     timestamptz not null default now()
);

create index integration_call_company_idx
    on integration_call (company_id, created_at desc);

create trigger integration_call_append_only
    before update or delete on integration_call
    for each row execute function forbid_ledger_mutation();
create trigger integration_call_no_truncate
    before truncate on integration_call
    for each statement execute function forbid_ledger_mutation();

grant select, insert on integration_call to regnmed_app;

create table integration_usage (
    integration_id uuid not null references integration (id),
    -- Bare selskapsrettede kall telles; det er de en admin kan se.
    company_id     uuid not null references company (id),
    dag            date not null,
    kall           bigint not null default 0,
    primary key (integration_id, company_id, dag)
);

grant select, insert on integration_usage to regnmed_app;
grant update (kall) on integration_usage to regnmed_app;
