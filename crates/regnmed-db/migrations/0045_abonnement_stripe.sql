-- Stripe Subscriptions for regnmeds EGET abonnement (docs/abonnement.md §5).
--
-- PRESISERINGEN som gjør denne migrasjonen mulig uten å bryte prinsippet
-- i #74: «ALDRI Stripe Billing» gjelder VÅR FAKTURAMOTOR — fakturaene
-- kundene våre sender til sine kunder. Abonnementet kundene betaler OSS
-- er en annen sak: der er Stripe leverandøren, og deres Subscription er
-- det som gjør at trekket gjentar seg til noen sier stopp. Vi vil ikke
-- være PCI-compliant, så kortdata skal ligge hos en som er det.
--
-- Det som IKKE flyttes til Stripe, og hvorfor:
--
--   * `abonnement` (dekningsradene) er fortsatt eneste kilde for
--     tilgangsvakten. prove/aktiv/frist/sperret beregnes i Postgres som
--     før — sperreregelen i docs/abonnement.md §1 er regnmed-oppførsel
--     ingen betalingsleverandør kjenner til.
--   * `abonnement_pris` er fortsatt autoritativ prisliste. Stripe-prisene
--     OPPRETTES FRA den, aldri omvendt; en prisendring er fortsatt en ny
--     datert rad med kilde.
--   * Bokføringen er vår. Hver betaling blir et bilag i driftsselskapets
--     hovedbok, uansett hvem som krevde inn pengene — bokføringsloven
--     bryr seg ikke om hvem som eier abonnementsobjektet.
--
-- To skinner går side om side med vilje: selskaper som alt har dekning
-- (migrasjon 0041 ga alle en) fortsetter på månedsjobben med faktura og
-- KID. Bare NYE tegninger går via Stripe. Ingen pilotkunde tvinges til å
-- taste kort på nytt for at vi skal bytte mekanikk.

-- Intervallet blir en del av prislisten, slik at årspris kan settes
-- uavhengig av månedsprisen (12 × månedspris er en antakelse, ikke en
-- lov — rabatt på årsbetaling er et prisvedtak, ikke en ganging).
alter table abonnement_pris
    add column interval text not null default 'month'
        check (interval in ('month', 'year'));

-- Unikheten må nå ta med intervallet: samme plan kan ha både en måneds-
-- og en årspris fra samme dato.
alter table abonnement_pris drop constraint abonnement_pris_plan_valid_from_key;
alter table abonnement_pris add constraint abonnement_pris_plan_interval_valid_from_key
    unique (plan, interval, valid_from);

-- Speilet av prislisten hos Stripe. INSERT-ONLY som prislisten selv:
-- en Stripe Price er uforanderlig etter opprettelse (deres regel, ikke
-- vår), så en prisendring gir en ny rad her også — og gamle abonnement
-- fortsetter på sin gamle Price til de flyttes. Det er grandfathering
-- gratis, samme egenskap som daterte prisrader gir oss ellers.
create table abonnement_stripe_price (
    id               uuid primary key default gen_random_uuid(),
    plan             text not null check (plan <> ''),
    interval         text not null check (interval in ('month', 'year')),
    -- BRUTTO i øre, altså inkl. mva. Stripe kjenner ikke norsk mva med
    -- mindre man skrur på Stripe Tax, og det vil vi ikke: vi kan satsen
    -- selv (satsregisteret), og en avgift beregnet to steder er en
    -- avgift som før eller siden spriker. Prisen kunden ser hos Stripe
    -- er derfor den de faktisk betaler, og splittes hos oss ved
    -- bokføring.
    brutto_ore       bigint not null check (brutto_ore > 0),
    stripe_price_id  text not null unique check (stripe_price_id <> ''),
    -- Hvilken rad i prislisten den ble laget fra, så sporet tilbake til
    -- prisvedtaket finnes.
    kilde            text not null check (kilde <> ''),
    created_at       timestamptz not null default now()
);

create index abonnement_stripe_price_plan_idx
    on abonnement_stripe_price (plan, interval, created_at desc);

grant select, insert on abonnement_stripe_price to regnmed_app;

create trigger abonnement_stripe_price_append_only
    before update or delete on abonnement_stripe_price
    for each row execute function forbid_ledger_mutation();
create trigger abonnement_stripe_price_no_truncate
    before truncate on abonnement_stripe_price
    for each statement execute function forbid_ledger_mutation();

-- Selskapets abonnement HOS STRIPE. Tilstand, ikke bevis: den kan endre
-- seg (plan, status, oppsigelse), og beviset for hver betaling ligger i
-- `kortbetaling` og i hovedboken.
--
-- `stripe_subscription_id` er unik: ett aktivt abonnement per selskap.
-- Sier kunden opp og tegner på nytt, er det en ny Subscription og en ny
-- rad — historikken består.
create table abonnement_stripe (
    id                     uuid primary key default gen_random_uuid(),
    company_id             uuid not null references company (id),
    stripe_subscription_id text not null unique check (stripe_subscription_id <> ''),
    stripe_price_id        text not null check (stripe_price_id <> ''),
    plan                   text not null check (plan <> ''),
    interval               text not null check (interval in ('month', 'year')),
    -- Stripes egen status (active, past_due, canceled, ...). Speilet for
    -- innsyn og feilsøking; TILGANGEN styres aldri av denne, bare av
    -- dekningsradene i `abonnement`.
    status                 text not null check (status <> ''),
    -- Satt når abonnementet er sagt opp men fortsatt løper ut perioden.
    cancel_at              timestamptz,
    canceled_at            timestamptz,
    created_at             timestamptz not null default now(),
    updated_at             timestamptz not null default now()
);

-- Ett AKTIVT abonnement per selskap. Oppsagte rader (canceled_at satt)
-- er historikk og teller ikke, så en ny tegning etter oppsigelse går
-- gjennom.
create unique index abonnement_stripe_ett_aktivt
    on abonnement_stripe (company_id) where canceled_at is null;

create index abonnement_stripe_company_idx
    on abonnement_stripe (company_id, created_at desc);

-- Status og oppsigelsesdatoene endres av webhooken; resten står fast.
grant select, insert on abonnement_stripe to regnmed_app;
grant update (status, cancel_at, canceled_at, updated_at) on abonnement_stripe to regnmed_app;

create trigger abonnement_stripe_no_delete
    before delete on abonnement_stripe
    for each row execute function forbid_ledger_mutation();
create trigger abonnement_stripe_no_truncate
    before truncate on abonnement_stripe
    for each statement execute function forbid_ledger_mutation();

-- `kortbetaling.invoice_id` pekte på en faktura som ALLTID fantes fra
-- før (månedsjobben lagde den, kortet betalte den etterpå). Med Stripe
-- kommer betalingen først og fakturaen lages i samme transaksjon, så
-- kolonnen trenger ikke endres — men vi trenger å vite hvilken
-- Stripe-faktura raden gjelder, slik at en webhook-replay kjennes igjen
-- selv før vår egen faktura finnes.
alter table kortbetaling add column stripe_invoice_id text;
create unique index kortbetaling_stripe_invoice_idx
    on kortbetaling (stripe_invoice_id) where stripe_invoice_id is not null;
