-- Satsregisteret: regelverksstyrte satser som data med gyldighetsperioder
-- (docs/regelverk.md). Same doctrine as vat_rate: the row says what the
-- value is and from when; lookups always ask "the sats valid on this
-- date"; a rule change is one reviewed INSERT carrying its legal source.
-- Append-only like all evidence — history must re-report identically
-- forever.
--
-- verdi is an integer in the domain's unit (enhet): 'bp' basis points,
-- 'ore' øre, 'ore_per_km' øre per kilometre.

create table sats (
    domene     text not null check (domene <> ''),
    valid_from date not null,
    verdi      bigint not null,
    enhet      text not null check (enhet in ('bp', 'ore', 'ore_per_km')),
    kilde      text not null check (kilde <> ''),
    created_at timestamptz not null default now(),
    primary key (domene, valid_from)
);

create trigger sats_append_only
    before update or delete on sats
    for each row execute function forbid_ledger_mutation();
create trigger sats_no_truncate
    before truncate on sats
    for each statement execute function forbid_ledger_mutation();

grant select, insert on sats to regnmed_app;

-- Seeded with verified values (fetched from the named sources
-- 2026-07-23). Domains start at their earliest VERIFIED date — no
-- guessed history, ever.

insert into sats (domene, valid_from, verdi, enhet, kilde) values
-- Forsinkelsesrenteloven §3: styringsrente + minst 8 pp, fastsatt hvert
-- halvår av Finanstilsynet.
('forsinkelsesrente', '2025-01-01', 1250, 'bp', 'Finanstilsynet, forsinkelsesrente 1. halvår 2025'),
('forsinkelsesrente', '2025-07-01', 1225, 'bp', 'Finanstilsynet, forsinkelsesrente 2. halvår 2025'),
('forsinkelsesrente', '2026-01-01', 1200, 'bp', 'Finanstilsynet / forskrift 2025-12-18-2658'),
('forsinkelsesrente', '2026-07-01', 1225, 'bp', 'Finanstilsynet, forsinkelsesrente 2. halvår 2026'),
-- Standardkompensasjon for inndrivelseskostnader (samme forskrift).
('standardkompensasjon', '2026-01-01', 46000, 'ore', 'Forskrift 2025-12-18-2658'),
('standardkompensasjon', '2026-07-01', 43000, 'ore', 'Finanstilsynet, standardkompensasjon 2. halvår 2026'),
-- Inkassosatsen (inkassoforskriften §1-1); maksimalt purregebyr er
-- 1/20 av inkassosatsen.
('inkassosats',     '2025-01-01', 70000, 'ore', 'Inkassoforskriften, sats 2025'),
('inkassosats',     '2026-01-01', 75000, 'ore', 'Finanstilsynet: inkassosatsen 2026'),
('purregebyr_maks', '2025-01-01',  3500, 'ore', '1/20 av inkassosatsen 2025'),
('purregebyr_maks', '2026-01-01',  3800, 'ore', '1/20 av inkassosatsen 2026'),
-- Statens sats for kjøregodtgjørelse og den trekkfrie delen
-- (særavtale + forskudds-/skattefastsettingsforskrift).
('km_godtgjorelse',          '2025-01-01', 500, 'ore_per_km', 'Statens sats 2025'),
('km_godtgjorelse',          '2026-01-01', 530, 'ore_per_km', 'Statens sats 2026'),
('km_godtgjorelse_trekkfri', '2025-01-01', 350, 'ore_per_km', 'Skattedirektoratet, trekkfri sats 2025'),
('km_godtgjorelse_trekkfri', '2026-01-01', 350, 'ore_per_km', 'Forskrift 2025-11-07-2216 (satser 2026)'),
-- Terskelverdier (endres sjelden; kadens overvåkes ikke).
('aktiveringsgrense',      '2024-01-01', 3000000, 'ore', 'Skatteloven §14-40, hevet fra 2024'),
('mva_registreringsgrense','2004-01-01', 5000000, 'ore', 'Merverdiavgiftsloven §2-1');
