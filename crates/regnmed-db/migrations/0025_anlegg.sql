-- Anleggsregister og avskrivninger (docs/anlegg.md, #40).
--
-- An asset's history is EVIDENCE: the register allows insert + the
-- one-way avhending transition and nothing else (trigger-enforced,
-- like the billing marker on time_entry). Bokført verdi is never
-- stored — it is kostpris − SUM over the depreciation log, computed
-- like every other balance.
--
-- Depreciation vouchers are generated system work with a human-visible
-- insert-only run log (the repeterende-faktura pattern): a partial
-- unique index makes an (asset, period) impossible to depreciate
-- twice; failures log a detail row and are retried by the next run.

create table asset (
    id                     uuid primary key,
    company_id             uuid not null references company (id),
    navn                   text not null check (navn <> ''),
    anskaffelsesdato       date not null,
    kostpris_ore           bigint not null check (kostpris_ore > 0),
    restverdi_ore          bigint not null default 0 check (restverdi_ore >= 0),
    levetid_maneder        integer not null check (levetid_maneder > 0),
    -- Account NUMBERS (copy-at-creation, like invoice lines): the
    -- balansekonto carries the asset, avskrivningskonto takes the cost.
    balansekonto           text not null check (balansekonto <> ''),
    avskrivningskonto      text not null check (avskrivningskonto <> ''),
    -- Skatteloven §14-41; rates live in the satsregister
    -- (saldogruppe_a … saldogruppe_j).
    saldogruppe            text not null
        check (saldogruppe in ('a','b','c','d','e','f','g','h','i','j')),
    -- The purchase voucher, when the anskaffelse was posted here.
    anskaffelse_voucher_id uuid references voucher (id),
    -- Avhending: set ONCE, together. The voucher is null only when
    -- there was nothing to post (fully depreciated, no vederlag).
    avhendet_dato          date,
    vederlag_ore           bigint check (vederlag_ore is null or vederlag_ore >= 0),
    avhending_voucher_id   uuid references voucher (id),
    created_by             text not null check (created_by <> ''),
    created_at             timestamptz not null default now(),
    check (restverdi_ore < kostpris_ore),
    check ((avhendet_dato is null) = (vederlag_ore is null)),
    check (avhendet_dato is not null or avhending_voucher_id is null),
    check (avhendet_dato is null or avhendet_dato >= anskaffelsesdato)
);

create index asset_company_idx on asset (company_id, anskaffelsesdato);

create function asset_guard() returns trigger
language plpgsql as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'assets are evidence and cannot be deleted (avhend instead)';
    end if;
    -- The ONLY allowed update is the one-way avhending transition:
    -- everything else identical, avhending fields set exactly once.
    if old.avhendet_dato is null and new.avhendet_dato is not null
       and new.id = old.id and new.company_id = old.company_id
       and new.navn = old.navn
       and new.anskaffelsesdato = old.anskaffelsesdato
       and new.kostpris_ore = old.kostpris_ore
       and new.restverdi_ore = old.restverdi_ore
       and new.levetid_maneder = old.levetid_maneder
       and new.balansekonto = old.balansekonto
       and new.avskrivningskonto = old.avskrivningskonto
       and new.saldogruppe = old.saldogruppe
       and new.anskaffelse_voucher_id is not distinct from old.anskaffelse_voucher_id
       and new.created_by = old.created_by
       and new.created_at = old.created_at then
        return new;
    end if;
    raise exception 'asset rows are immutable except the one-way avhending';
end;
$$;

create trigger asset_immutable
    before update or delete on asset
    for each row execute function asset_guard();
create trigger asset_no_truncate
    before truncate on asset
    for each statement execute function forbid_ledger_mutation();

grant select, insert on asset to regnmed_app;
grant update (avhendet_dato, vederlag_ore, avhending_voucher_id) on asset to regnmed_app;

create table asset_depreciation (
    id         uuid primary key,
    asset_id   uuid not null references asset (id),
    -- First day of the depreciated month; the voucher is dated the
    -- month's last day.
    period     date not null,
    amount_ore bigint not null check (amount_ore >= 0),
    voucher_id uuid references voucher (id),
    detail     text,
    created_at timestamptz not null default now(),
    -- A row is either a posted depreciation or a logged failure.
    check ((voucher_id is null) <> (detail is null))
);

create unique index asset_depreciation_once
    on asset_depreciation (asset_id, period) where voucher_id is not null;

create trigger asset_depreciation_append_only
    before update or delete on asset_depreciation
    for each row execute function forbid_ledger_mutation();
create trigger asset_depreciation_no_truncate
    before truncate on asset_depreciation
    for each statement execute function forbid_ledger_mutation();

grant select, insert on asset_depreciation to regnmed_app;

-- Saldogruppesatsene (skatteloven §14-43) into the satsregister —
-- regelverksdata with kilde, never hardcoded. Lovfestede satser that
-- change rarely: exempt from cadence monitoring like the thresholds.
-- Verified for 2025–2026; earliest verified date, no guessed history.
insert into sats (domene, valid_from, verdi, enhet, kilde) values
('saldogruppe_a', '2025-01-01', 3000, 'bp', 'Skatteloven §14-43 (1) a'),
('saldogruppe_b', '2025-01-01', 2000, 'bp', 'Skatteloven §14-43 (1) b'),
('saldogruppe_c', '2025-01-01', 2400, 'bp', 'Skatteloven §14-43 (1) c'),
('saldogruppe_d', '2025-01-01', 2000, 'bp', 'Skatteloven §14-43 (1) d'),
('saldogruppe_e', '2025-01-01', 1400, 'bp', 'Skatteloven §14-43 (1) e'),
('saldogruppe_f', '2025-01-01', 1200, 'bp', 'Skatteloven §14-43 (1) f'),
('saldogruppe_g', '2025-01-01',  500, 'bp', 'Skatteloven §14-43 (1) g'),
('saldogruppe_h', '2025-01-01',  400, 'bp', 'Skatteloven §14-43 (1) h'),
('saldogruppe_i', '2025-01-01',  200, 'bp', 'Skatteloven §14-43 (1) i'),
('saldogruppe_j', '2025-01-01', 1000, 'bp', 'Skatteloven §14-43 (1) j');
