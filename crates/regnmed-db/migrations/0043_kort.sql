-- Kortskinnen (#74, docs/abonnement.md §5).
--
-- Kort er standardveien for abonnementsbetaling: faktura+KID krever at
-- noen SER innbetalingen (bankfiler er manuelle til bank-API finnes),
-- kort via webhook har ingen slik luke. Prinsippet står: VÅR
-- fakturamotor er autoritativ — kortet er bare en raskere vei til
-- «betalt» på samme reskontropost, og leverandøren er utskiftbar.
--
-- `betalingskort` er tilstand (kortet kan byttes), IKKE bevis: bare
-- referanser (Stripe-kundens id, betalingsmetodens id) og visningsinfo.
-- Kortnummer finnes aldri hos oss — kunden taster hos Stripe.
create table betalingskort (
    company_id         uuid primary key references company (id),
    stripe_customer_id text not null check (stripe_customer_id <> ''),
    payment_method_id  text not null check (payment_method_id <> ''),
    brand              text not null default '',
    last4              text not null default '',
    aktiv              boolean not null default true,
    created_at         timestamptz not null default now(),
    updated_at         timestamptz not null default now()
);

grant select, insert, update on betalingskort to regnmed_app;

-- `kortbetaling` er BEVIS: insert-only logg over hvert trekk, og
-- unikheten på payment_intent_id er dedup-nøkkelen som gjør webhooken
-- idempotent — samme hendelse levert to ganger bokfører aldri to
-- ganger.
create table kortbetaling (
    id                uuid primary key,
    -- Kundeselskapet trekket gjelder (fakturaen bor i driftsselskapet).
    company_id        uuid not null references company (id),
    invoice_id        uuid not null references invoice (id),
    payment_intent_id text not null unique check (payment_intent_id <> ''),
    status            text not null check (status in ('succeeded', 'failed')),
    belop_ore         bigint not null,
    detail            text,
    created_at        timestamptz not null default now()
);

create index kortbetaling_company_idx on kortbetaling (company_id, created_at desc);

grant select, insert on kortbetaling to regnmed_app;

create trigger kortbetaling_append_only
    before update or delete on kortbetaling
    for each row execute function forbid_ledger_mutation();
create trigger kortbetaling_no_truncate
    before truncate on kortbetaling
    for each statement execute function forbid_ledger_mutation();
