-- Plattforminnstillinger (docs/auth.md §8): det systemadmin bestemmer
-- for hele plattformen — først ikonstilen i portalmenyen (låst globalt,
-- brukerbeslutning 2026-08-06; ingen brukeroverstyring).
--
-- Satsregister-mønsteret i miniatyr: innsettings-bare rader, nyeste rad
-- per nøkkel gjelder, hvem som satte den står på raden. En innstilling
-- kan dermed aldri endres i det stille — historikken ER raden.

create table platform_setting (
    id         uuid primary key default gen_random_uuid(),
    key        text not null check (key <> ''),
    value      text not null,
    set_by     uuid not null references person (id),
    created_at timestamptz not null default now()
);

create index platform_setting_key_idx on platform_setting (key, created_at desc);

grant select, insert on platform_setting to regnmed_app;

create trigger platform_setting_append_only
    before update or delete on platform_setting
    for each row execute function forbid_ledger_mutation();
create trigger platform_setting_no_truncate
    before truncate on platform_setting
    for each statement execute function forbid_ledger_mutation();
