-- Egendefinerte roller (#60, docs/auth.md).
--
-- De innebygde rollene dekker de vanlige tilfellene, men ikke «en som
-- bare fakturerer», «en controller uten lønn» eller «en lagermedarbeider».
-- Et selskap skal kunne sette sammen sine egne av rettighetene som
-- allerede finnes (#59), uten at vi rører koden.
--
-- De INNEBYGDE rollene seedes med vilje IKKE hit. De er definert i
-- `regnmed-api::tilgang` og blir der: to definisjoner av det samme er to
-- steder å drive fra hverandre, og et regnskapssystem har ikke råd til
-- at tilgang betyr én ting i koden og en annen i databasen. Denne
-- tabellen holder bare det et selskap har funnet på selv.

create table company_role (
    id         uuid primary key,
    company_id uuid not null references company (id),
    -- Navnet er NØKKELEN: company_member.role peker på det. Derfor er
    -- det permanent, av samme grunn som en dimensjonskode er det —
    -- kunne det endres, ville medlemskapene pekt på en rolle som ikke
    -- finnes lenger. Vil man ha et annet navn, lager man en ny rolle.
    navn       text not null check (navn <> ''),
    aktiv      boolean not null default true,
    created_at timestamptz not null default now(),
    created_by text not null check (created_by <> ''),
    unique (company_id, navn)
);

-- Navnene på de innebygde rollene er reservert: en egendefinert rolle
-- som het 'admin' ville skygget for den ekte, og oppslaget ville gitt
-- selskapets egen versjon i stedet.
alter table company_role add constraint company_role_navn_ikke_innebygd
    check (navn not in ('admin', 'bokforing', 'les', 'ansatt', 'revisor', 'ukjent'));

create function forbid_role_identity_change() returns trigger
language plpgsql as $$
begin
    if new.company_id is distinct from old.company_id
       or new.navn is distinct from old.navn then
        raise exception 'rollens identitet kan ikke endres (#60)';
    end if;
    return new;
end;
$$;

create trigger company_role_identity
    before update on company_role
    for each row execute function forbid_role_identity_change();

create trigger company_role_no_delete
    before delete on company_role
    for each row execute function forbid_ledger_mutation();

-- Rettighetene rollen gir. Navnene er slug-ene fra `Rett` i koden;
-- databasen kjenner dem ikke, og et navn koden ikke gjenkjenner blir
-- IGNORERT ved oppslag. Rulles en versjon tilbake som ikke kjenner en ny
-- rettighet, forsvinner den — den blir ikke til noe annet.
create table company_role_right (
    role_id uuid not null references company_role (id),
    rett    text not null check (rett <> ''),
    primary key (role_id, rett)
);

-- Til forskjell fra resten av systemet er DELETE tillatt her, og det er
-- et bevisst unntak: å ta en rettighet ut av en rolle er en helt vanlig
-- endring, ikke en omskriving av historikk. Sporet over hva som ble
-- endret ligger i company_role_change.
grant select, insert, delete on company_role_right to regnmed_app;
grant select, insert on company_role to regnmed_app;
grant update (aktiv) on company_role to regnmed_app;

create table company_role_change (
    id         uuid primary key,
    company_id uuid not null references company (id),
    role_id    uuid not null references company_role (id),
    endring    text not null check (
                   endring in ('opprettet', 'rettigheter_endret', 'deaktivert', 'reaktivert')),
    rettigheter text,
    utfort_av  uuid references person (id),
    created_at timestamptz not null default now()
);

create index company_role_change_idx
    on company_role_change (company_id, created_at desc);

create trigger company_role_change_append_only
    before update or delete on company_role_change
    for each row execute function forbid_ledger_mutation();
create trigger company_role_change_no_truncate
    before truncate on company_role_change
    for each statement execute function forbid_ledger_mutation();

grant select, insert on company_role_change to regnmed_app;

-- company_member.role kunne bare være de fire innebygde navnene. Nå kan
-- den også være navnet på en egendefinert rolle, og listen er ikke
-- lenger kjent for databasen.
--
-- Det svekker ikke vernet: oppslaget er FAIL-CLOSED. Et rollenavn koden
-- ikke kjenner igjen gir INGEN rettigheter (Rolle::Ukjent, #54), så en
-- ugyldig verdi her stenger ute i stedet for å slippe inn. Det var
-- allerede den egenskapen som bar vekten; check-constraint-en var et
-- ekstra belte.
alter table company_member drop constraint company_member_role_check;
alter table company_invitation drop constraint company_invitation_role_check;
