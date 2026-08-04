-- Prosjektet eier faktureringsreglene for timene (docs/timer.md,
-- brukerbeslutning 2026-08-04): fakturerbar-standarden står på
-- prosjektet, og timesatsen er DATA i satsregister-mønsteret — daterte,
-- innsettings-bare rader per prosjekt, med person_id for en persons
-- egen sats og null for prosjektets standard. Oppslaget spør alltid
-- «satsen som gjaldt på timeføringens dato»; en satsendring er én
-- INSERT, og historikken står for alltid (allerede førte timer bærer
-- satsen sin selv, i time_entry.timesats_ore).

-- Redigerbar metadata på linje med navnet (utenfor identitetstriggeren
-- fra 0018, samme mønster som party_id i 0047). Bare prosjekter kan
-- være fakturerbare som standard; en avdeling med standarden satt er en
-- modellfeil.
alter table dimension
    add column fakturerbar_default boolean not null default false;
alter table dimension add constraint dimension_fakturerbar_prosjekt_only
    check (not fakturerbar_default or kind = 'prosjekt');
grant update (fakturerbar_default) on dimension to regnmed_app;

create table prosjekt_sats (
    id           uuid primary key,
    company_id   uuid not null references company(id),
    dimension_id uuid not null references dimension(id),
    -- null = prosjektets standardsats; ellers personens sats i prosjektet.
    person_id    uuid references person(id),
    timesats_ore bigint not null check (timesats_ore >= 0),
    valid_from   date not null,
    created_by   text not null check (created_by <> ''),
    created_at   timestamptz not null default now()
);

-- Nyeste rad ≤ dato vinner; person foran standard.
create index prosjekt_sats_lookup
    on prosjekt_sats (dimension_id, person_id, valid_from desc);

create trigger prosjekt_sats_append_only
    before update or delete on prosjekt_sats
    for each row execute function forbid_ledger_mutation();
create trigger prosjekt_sats_no_truncate
    before truncate on prosjekt_sats
    for each statement execute function forbid_ledger_mutation();

grant select, insert on prosjekt_sats to regnmed_app;
