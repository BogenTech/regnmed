-- Abonnement (#65, docs/abonnement.md).
--
-- regnmed får en kasse. Modellen er minst mulig tilstand:
--
--   * `abonnement` sier hvilke perioder et selskap HAR dekning for —
--     daterte rader, nyeste dekkende rad gjelder (mva_terminordning-
--     mønsteret). Status (prøvetid/aktiv/frist/sperret) LAGRES ALDRI,
--     den beregnes av regnmed-core::abonnement.
--   * `abonnement_pris` er prislisten som data, datert og med kilde —
--     satsregister-mønsteret. En prisendring er en ny rad; fakturaen
--     bruker prisen som gjaldt på faktureringsdagen.
--   * `abonnement_faktura_run` gjør månedsfakturaen idempotent: unik
--     per (selskap, år, måned), raden og fakturaen skrives i samme
--     transaksjon (repeterende faktura-mønsteret).
--
-- Prinsipp, håndhevet i tilgangsvakten: et sperret abonnement stopper
-- SKRIVING, aldri lesing eller eksport. Hovedboken tas ikke som gissel.

create table abonnement (
    id         uuid primary key,
    company_id uuid not null references company (id),
    plan       text not null check (plan <> ''),
    valid_from date not null,
    -- EKSKLUSIV, som overalt ellers: dagen dekningen ikke lenger
    -- gjelder. NULL = til videre.
    valid_to   date check (valid_to is null or valid_to > valid_from),
    -- Avtale-/vedtaksreferansen: HVORFOR raden finnes (tegning,
    -- oppsigelse, migrasjon). Aldri tom — en dekning uten forklaring
    -- er et hull i sporet.
    note       text not null check (note <> ''),
    created_by text not null check (created_by <> ''),
    created_at timestamptz not null default now()
);

create index abonnement_company_idx on abonnement (company_id, valid_from desc);

-- Applikasjonsrollen kan tegne og avslutte (sette valid_to), aldri
-- omskrive historikk eller slette — engagement-mønsteret.
grant select, insert on abonnement to regnmed_app;
grant update (valid_to) on abonnement to regnmed_app;

create trigger abonnement_no_delete
    before delete on abonnement
    for each row execute function forbid_ledger_mutation();
create trigger abonnement_no_truncate
    before truncate on abonnement
    for each statement execute function forbid_ledger_mutation();

-- Prislisten. Global (ikke per selskap), insert-only.
create table abonnement_pris (
    id               uuid primary key default gen_random_uuid(),
    plan             text not null check (plan <> ''),
    -- Per måned, eks. mva, i øre. Heltall som alle andre beløp.
    pris_ore_per_mnd bigint not null check (pris_ore_per_mnd >= 0),
    valid_from       date not null,
    kilde            text not null check (kilde <> ''),
    created_at       timestamptz not null default now(),
    unique (plan, valid_from)
);

grant select, insert on abonnement_pris to regnmed_app;

create trigger abonnement_pris_append_only
    before update or delete on abonnement_pris
    for each row execute function forbid_ledger_mutation();
create trigger abonnement_pris_no_truncate
    before truncate on abonnement_pris
    for each statement execute function forbid_ledger_mutation();

-- Startprisen. FORSLAG dokumentert i docs/abonnement.md — endres ved
-- prisvedtak med en ny rad, aldri ved å røre denne.
insert into abonnement_pris (plan, pris_ore_per_mnd, valid_from, kilde)
values ('standard', 24900, '2026-07-01',
        'innføringspris (#65) — forslag, erstattes av prisvedtak');

-- Kjøringsloggen for abonnementsfakturaene. Én faktura per selskap og
-- måned, garantert av unikheten; raden og fakturaen skrives i samme
-- transaksjon, så en feilet fakturering etterlater ingen rad og prøves
-- igjen neste kjøring.
create table abonnement_faktura_run (
    id         uuid primary key,
    -- KUNDESELSKAPET det faktureres for (fakturaen selv bor i
    -- driftsselskapets hovedbok).
    company_id uuid not null references company (id),
    ar         int not null,
    maned      int not null check (maned between 1 and 12),
    invoice_id uuid not null,
    created_at timestamptz not null default now(),
    unique (company_id, ar, maned)
);

grant select, insert on abonnement_faktura_run to regnmed_app;

create trigger abonnement_faktura_run_append_only
    before update or delete on abonnement_faktura_run
    for each row execute function forbid_ledger_mutation();
create trigger abonnement_faktura_run_no_truncate
    before truncate on abonnement_faktura_run
    for each statement execute function forbid_ledger_mutation();

-- Selskaper som finnes ved innføringen fortsetter uten avbrudd: de får
-- en åpen dekning fra i dag. Å la dem falle rett i «prøvetid utløpt»
-- ville sperret pilotkundene i samme øyeblikk som migrasjonen kjørte.
insert into abonnement (id, company_id, plan, valid_from, note, created_by)
select gen_random_uuid(), id, 'standard', current_date,
       'innført med migrasjon 0041 — eksisterende selskap fortsetter uten avbrudd',
       'migrasjon 0041'
from company;
