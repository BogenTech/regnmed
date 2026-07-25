-- Budsjett (docs/budsjett.md, #41): plan mot virkelighet, på tallene vi
-- allerede stoler på.
--
-- Et budsjett er et ARBEIDSDOKUMENT mens det er utkast — fritt
-- redigerbart, linjer kan legges til og fjernes. Det er ikke bevis, og
-- later ikke som det. Når noen FASTSETTER det, fryses det: raden og
-- linjene blir uforanderlige (trigger), og en revisjon er en NY VERSJON
-- for samme år. Derfor kan en avviksrapport alltid navngi nøyaktig
-- hvilket budsjett den sammenligner mot — «budsjett 2026 v2, fastsatt
-- av Lise 3. januar», ikke «budsjettet» som stille har endret seg siden
-- forrige gang noen så på det.
--
-- Beløpene er RESULTATKONTOER i PRESENTASJONSFORTEGN (som i
-- resultatrapporten: inntekt positiv, kostnad positiv) — et budsjett
-- skrives slik et menneske leser det, og avviksrapporten sammenligner i
-- samme rom. Hovedbokens debet/kredit-konvensjon gjelder bilag; dette
-- er ikke bilag.

create table budget (
    id           uuid primary key,
    company_id   uuid not null references company (id),
    year         integer not null check (year between 1900 and 2999),
    versjon      integer not null check (versjon >= 1),
    navn         text not null check (navn <> ''),
    status       text not null default 'utkast' check (status in ('utkast', 'fastsatt')),
    note         text,
    created_by   text not null check (created_by <> ''),
    created_at   timestamptz not null default now(),
    fastsatt_by  text,
    fastsatt_at  timestamptz,
    check (status <> 'fastsatt' or (fastsatt_by is not null and fastsatt_at is not null)),
    unique (company_id, year, versjon)
);

create index budget_company_idx on budget (company_id, year, versjon desc);

create function budget_guard() returns trigger
language plpgsql as $$
begin
    if tg_op = 'DELETE' then
        -- Utkast kan forkastes; et fastsatt budsjett er historikk.
        if old.status = 'fastsatt' then
            raise exception 'fastsatt budsjett % kan ikke slettes', old.id;
        end if;
        return old;
    end if;
    if old.status = 'fastsatt' then
        raise exception 'budsjett % er fastsatt og kan ikke endres', old.id;
    end if;
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.year is distinct from old.year
       or new.versjon is distinct from old.versjon
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'budsjettets identitet er uforanderlig';
    end if;
    if new.status = 'utkast' and old.status = 'utkast' then
        return new;
    end if;
    if new.status = 'fastsatt' and old.status = 'utkast' then
        return new;
    end if;
    raise exception 'budsjett % kan ikke gå fra % til %', old.id, old.status, new.status;
end;
$$;

create trigger budget_change_guard
    before update or delete on budget
    for each row execute function budget_guard();
create trigger budget_no_truncate
    before truncate on budget
    for each statement execute function forbid_ledger_mutation();

grant select, insert, delete on budget to regnmed_app;
grant update (navn, note, status, fastsatt_by, fastsatt_at) on budget to regnmed_app;

create table budget_line (
    id         uuid primary key,
    budget_id  uuid not null references budget (id) on delete cascade,
    -- Bare resultatkontoer: et resultatbudsjett budsjetterer resultat.
    -- (Likviditetsbudsjett er bevisst utenfor v1.)
    account_id uuid not null references account (id),
    maned      integer not null check (maned between 1 and 12),
    belop_ore  bigint not null,
    unique (budget_id, account_id, maned)
);

create index budget_line_budget_idx on budget_line (budget_id);

create function budget_line_guard() returns trigger
language plpgsql as $$
declare
    parent_status text;
    parent_id uuid;
begin
    parent_id := case tg_op when 'DELETE' then old.budget_id else new.budget_id end;
    select status into parent_status from budget where id = parent_id;
    if parent_status = 'fastsatt' then
        raise exception 'budsjettet er fastsatt — linjene kan ikke endres';
    end if;
    return case tg_op when 'DELETE' then old else new end;
end;
$$;

create trigger budget_line_change_guard
    before insert or update or delete on budget_line
    for each row execute function budget_line_guard();
create trigger budget_line_no_truncate
    before truncate on budget_line
    for each statement execute function forbid_ledger_mutation();

grant select, insert, update, delete on budget_line to regnmed_app;
