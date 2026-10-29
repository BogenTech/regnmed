-- Å gi en agent en oppgave (#103).
--
-- En agent kan nå holde en rolle (#101) og har verktøy å jobbe med
-- (#102). Det som manglet var det enkleste: en måte å SI HVA DEN SKAL
-- GJØRE på, som ikke er en melding i en chat eieren mister.
--
-- Formen er den samme som resten av systemet, og av samme grunn:
--
--   agent_oppgave            selve bestillingen. Uforanderlig når den er
--                            gitt — en endret oppgave er en ny oppgave,
--                            ellers kan ingen etterpå se hva agenten
--                            faktisk ble bedt om.
--   agent_oppgave_hendelse   innsettings-bart spor. Nyeste hendelse
--                            gjelder; ingen hendelse betyr 'ny'.
--
-- Statusen BEREGNES, lagres aldri — samme regel som abonnementsstatus
-- og saldo. 'utfort' og 'avvist' er ENDELIGE: triggeren nekter en
-- hendelse på en avsluttet oppgave, så et svar kan ikke trekkes tilbake
-- i stillhet. 'sporsmal' er med vilje IKKE endelig: en agent som er
-- usikker skal kunne spørre og fortsette, for alternativet er at den
-- gjetter — og #34/#103s doktrine er at den aldri skal gjette.
--
-- Oppgaven kan stilles til en bestemt integrasjon eller til ingen
-- spesiell (NULL = «den agenten som har tilgang»). Den som svarer står
-- i hendelsen, så en oppgave uten adressat likevel har et navn på den
-- som tok den.

create table agent_oppgave (
    id             uuid primary key,
    company_id     uuid not null references company (id),
    -- NULL = enhver agent med tilgang kan ta den.
    integration_id uuid references integration (id),
    melding        text not null check (melding <> ''),
    frist          date,
    opprettet_av   text not null check (opprettet_av <> ''),
    created_at     timestamptz not null default now()
);

create index agent_oppgave_company_idx on agent_oppgave (company_id, created_at desc);

create trigger agent_oppgave_append_only
    before update or delete on agent_oppgave
    for each row execute function forbid_ledger_mutation();
create trigger agent_oppgave_no_truncate
    before truncate on agent_oppgave
    for each statement execute function forbid_ledger_mutation();

grant select, insert on agent_oppgave to regnmed_app;

create table agent_oppgave_hendelse (
    id         uuid primary key,
    oppgave_id uuid not null references agent_oppgave (id),
    status     text not null check (status in ('pagar', 'sporsmal', 'utfort', 'avvist')),
    -- Begrunnelsen. Et avvist eller spørrende svar uten tekst er ikke et
    -- svar, så den er obligatorisk der.
    notat      text,
    av         text not null check (av <> ''),
    created_at timestamptz not null default now(),
    check (status in ('pagar', 'utfort') or (notat is not null and notat <> ''))
);

create index agent_oppgave_hendelse_idx
    on agent_oppgave_hendelse (oppgave_id, created_at desc);

create function agent_oppgave_hendelse_guard() returns trigger
language plpgsql as $$
declare siste text;
begin
    select status into siste from agent_oppgave_hendelse
     where oppgave_id = new.oppgave_id
     order by created_at desc, id desc limit 1;
    if siste in ('utfort', 'avvist') then
        raise exception
            'oppgaven er avsluttet (%) — et svar kan ikke trekkes tilbake, og en ny bestilling er en ny oppgave',
            siste;
    end if;
    return new;
end;
$$;

create trigger agent_oppgave_hendelse_endelig
    before insert on agent_oppgave_hendelse
    for each row execute function agent_oppgave_hendelse_guard();
create trigger agent_oppgave_hendelse_append_only
    before update or delete on agent_oppgave_hendelse
    for each row execute function forbid_ledger_mutation();
create trigger agent_oppgave_hendelse_no_truncate
    before truncate on agent_oppgave_hendelse
    for each statement execute function forbid_ledger_mutation();

grant select, insert on agent_oppgave_hendelse to regnmed_app;
