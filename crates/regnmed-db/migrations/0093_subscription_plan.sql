-- Packages (plans) as data, editable from the platform console.
--
-- `abonnement_pris` (0041/0045) already holds the PRICES as dated,
-- append-only rows. What was missing is the package itself: its name,
-- who it is for, its size limits and what the public price page lists.
-- This is the same pattern again: insert-only versions, the newest row
-- per slug is what applies, `set_by` names who changed it, and nothing
-- is ever rewritten, so the history of what a package promised is the
-- table.
--
-- The limits (`voucher_limit`, `employee_limit`) are DESCRIPTIVE for now:
-- nothing meters or enforces them yet (docs/abonnement.md, #279). They
-- exist so the price page and the console say the same thing, and so the
-- day enforcement is built it reads the number the customer was shown.
--
-- `listed` decides whether the public price page shows the plan; a plan
-- can exist (and be billed) without being listed. A plan is "in force"
-- when `abonnement_pris` has a month price valid today; one whose first
-- price is dated in the future is shown as upcoming.

create table subscription_plan (
    id             uuid primary key default gen_random_uuid(),
    slug           text not null check (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' and length(slug) <= 40),
    name           text not null check (name <> '' and length(name) <= 80),
    audience       text not null default '' check (length(audience) <= 200),
    -- 'plan' is the package a company is on; 'addon' (tilleggstjeneste) is
    -- bought on top of one. The first add-on is `bank` (#70).
    kind           text not null default 'plan' check (kind in ('plan', 'addon')),
    -- An add-on that is not available yet is listed as "coming" and cannot
    -- be bought. Always true for plans.
    available      boolean not null default true,
    featured       boolean not null default false,
    listed         boolean not null default false,
    voucher_limit  integer check (voucher_limit is null or voucher_limit > 0),
    employee_limit integer check (employee_limit is null or employee_limit >= 0),
    -- A JSON array of short strings, shown as the plan's bullet list.
    features       jsonb not null default '[]'
                   check (jsonb_typeof(features) = 'array'
                          and jsonb_array_length(features) <= 12),
    sort           integer not null default 0,
    -- NULL = seeded by a migration (no person did it).
    set_by         uuid references person (id),
    created_at     timestamptz not null default now()
);

create index subscription_plan_slug_idx on subscription_plan (slug, created_at desc);

grant select, insert on subscription_plan to regnmed_app;

create trigger subscription_plan_append_only
    before update or delete on subscription_plan
    for each row execute function forbid_ledger_mutation();
create trigger subscription_plan_no_truncate
    before truncate on subscription_plan
    for each statement execute function forbid_ledger_mutation();

-- The add-on that is planned but not built: direct bank connection (#70).
-- Listed as "coming", no price yet: a price is entered when the decision
-- is made, and the add-on becomes available when it works.
-- The two plans that exist and are billed today (prisvedtak 2026-07-28).
insert into subscription_plan (slug, name, kind, available, audience, featured, listed, features, sort)
values
    ('basis', 'Basis', 'plan', true, 'Selvbetjent', false, true,
     '["Alle funksjoner", "Dokumentasjon som support"]', 10),
    ('standard', 'Standard', 'plan', true, 'Med e-postsupport', true, true,
     '["Alle funksjoner", "Support på e-post"]', 20),
    ('bank', 'Bankkobling', 'addon', false,
     'Henter bankbevegelsene automatisk i stedet for fra fil', false, true,
     '["Daglig henting av kontobevegelser", "Går inn i samme avstemming som filimport"]', 100);
