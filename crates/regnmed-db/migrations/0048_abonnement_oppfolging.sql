-- Automatisk abonnementsoppfølging (#75, docs/abonnement.md §5.3).
--
-- Maskinen sender, purrer og sperrer — og da må maskinen kunne SVARE
-- FOR SEG. Sendingene står allerede i utsendelsesloggen og purringene i
-- invoice_reminder; det som mangler et spor er BESLUTNINGENE om dekning:
-- at en dekning ble avsluttet for mislighold, og at den ble gjenopprettet
-- da betalingen kom.
--
-- Sporet er også maskinens eget minne: gjenoppretting skal BARE skje
-- for dekninger maskinen selv avsluttet for mislighold. En oppsigelse
-- ser identisk ut i abonnement-tabellen (valid_to satt) — uten dette
-- sporet ville en betalt sluttfaktura vekket et oppsagt abonnement til
-- live igjen, for alltid.

create table abonnement_oppfolging (
    id         uuid primary key,
    -- Kundeselskapet det gjelder (ikke driftsselskapet).
    company_id uuid not null references company (id),
    -- Abonnementsfakturaen som utløste beslutningen, når det finnes én.
    invoice_id uuid,
    aksjon     text not null check (aksjon in ('sperret', 'gjenopprettet')),
    detail     text not null check (detail <> ''),
    created_at timestamptz not null default now()
);

create index abonnement_oppfolging_idx
    on abonnement_oppfolging (company_id, created_at desc);

create trigger abonnement_oppfolging_append_only
    before update or delete on abonnement_oppfolging
    for each row execute function forbid_ledger_mutation();
create trigger abonnement_oppfolging_no_truncate
    before truncate on abonnement_oppfolging
    for each statement execute function forbid_ledger_mutation();

grant select, insert on abonnement_oppfolging to regnmed_app;
