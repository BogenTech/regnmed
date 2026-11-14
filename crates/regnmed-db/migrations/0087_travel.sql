-- Business travel: diett and nattillegg (docs/utlegg.md «Reise og diett»).
--
-- A travel claim ('reise') is the reiseregning: departure and return to
-- the minute, where the traveller was each døgn, where they slept and
-- which meals someone else paid for. It gathers the trip's costs (hotel,
-- flights, taxi: utlegg) and its drives (kjøring) under trip_expense_id.
--
-- Two rate sets meet on every døgn:
-- - TREKKFRI (satsforskriften 2026, FOR-2025-11-07-2216 §§ 2-3): the same
--   in Norway and abroad;
-- - what the company PAYS, chosen per company: those same rates, or the
--   state's (særavtale reise innenlands §§ 9-10; utenfor Norge § 8 with
--   its per-country table, travel_rate_abroad). The excess over the
--   trekkfri rate is trekkpliktig and paid with pay.
--
-- At submission the computed døgn lines (travel_lines), the scheme and the
-- foreign rates and exchange rates they used are stored: the row is
-- evidence, like the km rate on a kjøring.

alter table expense drop constraint expense_kind_check;
alter table expense add constraint expense_kind_check
    check (kind in ('utlegg', 'kjoring', 'reise'));

-- A trip's costs and drives hang under it; a drive's tolls under the drive.
alter table expense drop constraint expense_trip_check;
alter table expense add constraint expense_trip_check
    check (trip_expense_id is null or kind in ('utlegg', 'kjoring'));

alter table expense
    add column departure_at timestamp,
    add column return_at timestamp,
    add column destination text,
    add column overnight boolean not null default false,
    -- The traveller's confirmation that the trip went at least 15 km from
    -- where they start (satsforskriften § 3, særavtale innenlands § 9).
    add column over_15_km boolean not null default false,
    -- What the traveller enters per døgn: [{place, lodging, breakfast,
    -- lunch, dinner}], place = 'NO' or a travel_rate_abroad key.
    add column travel_days jsonb not null default '[]',
    add column rate_scheme text check (rate_scheme in ('trekkfri', 'statens')),
    -- Computed at submission, per døgn: amounts paid and trekkfri, and the
    -- foreign rate and exchange rate used.
    add column travel_lines jsonb,
    add constraint expense_travel_kind_check
        check (kind = 'reise' or (departure_at is null and return_at is null
                                  and travel_lines is null)),
    add constraint expense_travel_check
        check (kind <> 'reise' or status in ('utkast', 'forkastet')
               or (departure_at is not null and return_at > departure_at
                   and over_15_km and rate_scheme is not null
                   and travel_lines is not null
                   and trekkfri_ore is not null and trekkpliktig_ore is not null));

create or replace function expense_guard() returns trigger
language plpgsql as $$
begin
    if is_content_offload(to_jsonb(old), to_jsonb(new), 'receipt_content', 'receipt_sha256') then
        return new;
    end if;
    -- Identity never moves, draft or not.
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.person_id is distinct from old.person_id
       or new.registered_by_person is distinct from old.registered_by_person
       or new.kind is distinct from old.kind
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'expense identity is immutable';
    end if;
    if old.status = 'utkast' then
        -- A draft has no decision trail, and gets none on the way in.
        if new.decided_by is not null or new.decided_at is not null
           or new.avvist_note is not null or new.voucher_id is not null
           or new.motkonto is not null or new.utbetalt_voucher_id is not null
           or new.utbetalt_at is not null then
            raise exception 'expense % is a draft and has no decision', old.id;
        end if;
        if new.status in ('utkast', 'innsendt', 'forkastet') then
            return new;
        end if;
        raise exception 'expense % kan ikke gå fra utkast til %', old.id, new.status;
    end if;
    -- From submission on: the content is evidence.
    if new.dato is distinct from old.dato
       or new.beskrivelse is distinct from old.beskrivelse
       or new.belop_ore is distinct from old.belop_ore
       or new.receipt_filename is distinct from old.receipt_filename
       or new.receipt_content_type is distinct from old.receipt_content_type
       or new.receipt_content is distinct from old.receipt_content
       or new.receipt_sha256 is distinct from old.receipt_sha256
       or new.km is distinct from old.km
       or new.sats_ore_per_km is distinct from old.sats_ore_per_km
       or new.trekkfri_ore is distinct from old.trekkfri_ore
       or new.trekkpliktig_ore is distinct from old.trekkpliktig_ore
       or new.department is distinct from old.department
       or new.project is distinct from old.project
       or new.billable is distinct from old.billable
       or new.route_from is distinct from old.route_from
       or new.route_to is distinct from old.route_to
       or new.round_trip is distinct from old.round_trip
       or new.vehicle is distinct from old.vehicle
       or new.passengers is distinct from old.passengers
       or new.passenger_rate_ore_per_km is distinct from old.passenger_rate_ore_per_km
       or new.trip_expense_id is distinct from old.trip_expense_id
       or new.departure_at is distinct from old.departure_at
       or new.return_at is distinct from old.return_at
       or new.destination is distinct from old.destination
       or new.overnight is distinct from old.overnight
       or new.over_15_km is distinct from old.over_15_km
       or new.travel_days is distinct from old.travel_days
       or new.rate_scheme is distinct from old.rate_scheme
       or new.travel_lines is distinct from old.travel_lines then
        raise exception 'expense content is immutable from submission';
    end if;
    if old.status = 'innsendt' and new.status in ('godkjent', 'avvist') then
        return new;
    end if;
    if old.status = 'godkjent' and new.status = 'utbetalt'
       and new.decided_by = old.decided_by and new.decided_at = old.decided_at
       and new.voucher_id = old.voucher_id and new.motkonto = old.motkonto then
        return new;
    end if;
    raise exception 'expense % kan ikke gå fra % til %', old.id, old.status, new.status;
end;
$$;

grant update (departure_at, return_at, destination, overnight, over_15_km, travel_days,
              rate_scheme, travel_lines)
    on expense to regnmed_app;

-- Whose rates the company pays. Stored on each claim at submission too, so
-- changing it never touches a submitted claim.
alter table company add column travel_rate_scheme text not null default 'trekkfri'
    check (travel_rate_scheme in ('trekkfri', 'statens'));
grant update (travel_rate_scheme) on company to regnmed_app;

-- Domestic rates, øre per døgn. Fetched 2026-11-13.
insert into sats (domene, valid_from, verdi, enhet, kilde) values
('diett_hotell_trekkfri',      '2026-01-01',  69300, 'ore', 'Satsforskriften 2026 (FOR-2025-11-07-2216) § 2: hotell, frokost betalt selv'),
('diett_pensjonat_trekkfri',   '2026-01-01',  40000, 'ore', 'Satsforskriften 2026 § 2: hybel uten kokemuligheter, pensjonat, brakke'),
('diett_privat_trekkfri',      '2026-01-01',  10700, 'ore', 'Satsforskriften 2026 § 2: hybel med kokemuligheter, privat'),
('diett_dag_6_12_trekkfri',    '2026-01-01',  20000, 'ore', 'Satsforskriften 2026 § 3: uten overnatting, 6-12 timer, minst 15 km'),
('diett_dag_over_12_trekkfri', '2026-01-01',  40000, 'ore', 'Satsforskriften 2026 § 3: uten overnatting, over 12 timer, minst 15 km'),
('nattillegg_trekkfri',        '2026-01-01',  45200, 'ore', 'Satsforskriften 2026 § 2: ulegitimert nattillegg etter statens regulativ, innenlands'),
('diett_dag_6_12',             '2026-01-01',  39700, 'ore', 'Særavtale reise innenlands 2026-2027 § 9 nr. 1 b'),
('diett_dag_over_12',          '2026-01-01',  73600, 'ore', 'Særavtale reise innenlands 2026-2027 § 9 nr. 1 c'),
('diett_overnatting',          '2026-01-01', 101200, 'ore', 'Særavtale reise innenlands 2026-2027 § 9 nr. 1 a'),
('nattillegg',                 '2026-01-01',  45200, 'ore', 'Særavtale reise innenlands 2026-2027 § 10 nr. 2');

-- The state's kost rates abroad, per country and for some per city, in the
-- currency the table gives (docs/reise/). Dated and append-only like the
-- satsregister; a new year is new rows.
create table travel_rate_abroad (
    region       text not null,
    country      text not null check (country <> ''),
    iso3         text check (iso3 ~ '^[A-Z]{3}$'),
    -- '' = the whole country.
    area         text not null default '',
    -- Hundredths of the currency's unit.
    amount_minor bigint not null check (amount_minor > 0),
    currency     text not null check (currency ~ '^[A-Z]{3}$'),
    valid_from   date not null default '2026-01-01',
    source       text not null check (source <> ''),
    created_at   timestamptz not null default now(),
    primary key (country, area, valid_from)
);

create trigger travel_rate_abroad_append_only
    before update or delete on travel_rate_abroad
    for each row execute function forbid_ledger_mutation();
create trigger travel_rate_abroad_no_truncate
    before truncate on travel_rate_abroad
    for each statement execute function forbid_ledger_mutation();

grant select on travel_rate_abroad to regnmed_app;

insert into travel_rate_abroad (region, country, iso3, area, amount_minor, currency, source) values
('Europa', 'Albania', 'ALB', 'Tirana', 52800, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Østerrike', 'AUT', 'Utenom Wien', 8100, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Østerrike', 'AUT', 'Wien', 8500, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Belarus', 'BLR', 'Minsk', 12600, 'BYN', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Belgia', 'BEL', 'Brüssel', 10200, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Bosnia-Hercegovina', 'BIH', 'Sarajevo', 55100, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Bulgaria', 'BGR', 'Sofia', 5700, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Kroatia', 'HRV', 'Zagreb', 6800, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Kypros', 'CYP', 'Nicosia', 7200, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Tsjekkia', 'CZE', 'Praha', 197000, 'CZK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Danmark', 'DNK', 'København', 99400, 'DKK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Estland', 'EST', 'Tallinn', 8200, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Finland', 'FIN', 'Helsingfors', 11400, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Frankrike', 'FRA', 'Utenom Paris', 8600, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Frankrike', 'FRA', 'Paris', 9300, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Tyskland', 'DEU', 'Berlin', 8700, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Tyskland', 'DEU', 'Utenom Berlin', 8400, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Hellas', 'GRC', 'Athen', 8200, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Ungarn', 'HUN', 'Budapest', 3196000, 'HUF', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Island', 'ISL', 'Reykjavik', 1813600, 'ISK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Irland', 'IRL', 'Dublin', 11700, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Italia', 'ITA', 'Roma', 9300, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Kosovo', 'XXK', 'Pristina', 3800, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Latvia', 'LVA', 'Riga', 9000, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Litauen', 'LTU', 'Vilnius', 7800, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Liechtenstein', 'LIE', 'Vaduz', 13500, 'CHF', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Luxembourg', 'LUX', 'Luxembourg', 10800, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Nord-Makedonia', 'MKD', 'Skopje', 49900, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Malta', 'MLT', 'Malta', 8100, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Moldova', 'MDA', 'Chisinau', 62500, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Monaco', 'MCO', 'Monaco', 11600, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Montenegro', 'MNE', 'Podgorica', 5200, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Nederland', 'NLD', 'Amsterdam', 8500, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Polen', 'POL', 'Warszawa', 29700, 'PLN', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Portugal', 'PRT', 'Lisboa', 7500, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Romania', 'ROU', 'Bucuresti', 26900, 'RON', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Russland', 'RUS', 'Utenom Moskva og St. Petersburg', 57900, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Russland', 'RUS', 'Moskva', 99900, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Russland', 'RUS', 'St Petersburg', 77100, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Serbia', 'SRB', 'Beograd', 76000, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Slovakia', 'SVK', 'Bratislava', 8700, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Slovenia', 'SVN', 'Ljubljana', 7100, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Spania', 'ESP', 'Madrid', 7700, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Sverige', 'SWE', 'Stockholm', 101200, 'SEK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Sveits', 'CHE', 'Utenom Genève og Zurich', 13500, 'CHF', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Sveits', 'CHE', 'Genève', 13500, 'CHF', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Sveits', 'CHE', 'Zurich', 14300, 'CHF', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Ukraina', 'UKR', 'Kiev', 49400, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Storbritannia', 'GBR', 'London', 9000, 'GBP', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Storbritannia', 'GBR', 'Utenom London', 7800, 'GBP', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Algerie', 'DZA', 'Algiers', 52800, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Angola', 'AGO', 'Luanda', 80700, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Benin', 'BEN', 'Cotonou', 79800, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Botswana', 'BWA', 'Gaborone', 57000, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Burundi', 'BDI', 'Bujumbura', 66200, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Kamerun', 'CMR', 'Yaounde', 73600, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Kongo-Brazzaville', 'COG', 'Brazzaville', 102300, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Elfenbenskysten', 'CIV', 'Abidjan', 103900, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Kongo', 'COD', 'Kinshasa', 105100, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Egypt', 'EGY', 'Kairo', 41200, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Eritrea', 'ERI', 'Asmara', 65200, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Eswatini', 'SWZ', 'Mbabane', 44400, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Etiopia', 'ETH', 'Addis Ababa', 33800, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Gabon', 'GAB', 'Libreville', 91500, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Ghana', 'GHA', 'Accra', 88500, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Kenya', 'KEN', 'Nairobi', 74300, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Lesotho', 'LSO', 'Maseru', 40300, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Madagaskar', 'MDG', 'Antananarivo', 40400, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Malawi', 'MWI', 'Lilongwe', 49600, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Mauritius', 'MUS', 'Port Louis', 54700, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Marokko', 'MAR', 'Rabat', 73800, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Mosambik', 'MOZ', 'Maputo', 62400, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Namibia', 'NAM', 'Windhoek', 47300, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Nigeria', 'NGA', 'Abuja', 65300, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Seychellene', 'SYC', 'Victoria', 104600, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Sør-Afrika', 'ZAF', 'Pretoria', 95100, 'ZAR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Sør-Sudan', 'SSD', 'Juba', 117300, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Sudan', 'SDN', 'Khartoum', 68500, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Tanzania', 'TZA', 'Dar es Salaam', 50400, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Togo', 'TGO', 'Lome', 76600, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Zambia', 'ZMB', 'Lusaka', 49100, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Zimbabwe', 'ZWE', 'Harare', 76100, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Afrika', 'Afrika øvrige', null, '', 68500, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Argentina', 'ARG', 'Buenos Aires', 73000, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Bahamas', 'BHS', 'Nassau', 160000, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Barbados', 'BRB', 'Bridgetown', 129300, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Bolivia', 'BOL', 'La Paz', 61400, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Brasil', 'BRA', 'Brasilia', 32300, 'BRL', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Brasil', 'BRA', 'Rio de Janeiro', 36000, 'BRL', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Canada', 'CAN', 'Ottawa', 17200, 'CAD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Chile', 'CHL', 'Santiago', 71400, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Colombia', 'COL', 'Bogota', 56600, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Costa Rica', 'CRI', 'San Jose', 91100, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Cuba', 'CUB', 'Havanna', 83900, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'El Salvador', 'SLV', 'San Salvador', 6800, 'USD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Guatemala', 'GTM', 'Guatemala by', 83600, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Guyana', 'GUY', 'Georgetown', 98700, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Honduras', 'HND', 'Tegucigalpa', 65900, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Jamaica', 'JAM', 'Kingston', 65800, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Mexico', 'MEX', 'Mexico by', 152000, 'MXN', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Nicaragua', 'NIC', 'Managua', 72700, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Panama', 'PAN', 'Panama by', 9200, 'USD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Paraguay', 'PRY', 'Asuncion', 46600, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Peru', 'PER', 'Lima', 69200, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Puerto Rico', 'PRI', 'San Juan', 9700, 'USD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Surinam', 'SUR', 'Paramaribo', 66400, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'USA', 'USA', 'Manhattan, NY', 14000, 'USD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'USA', 'USA', 'San Francisco,CA', 14500, 'USD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'USA', 'USA', 'Utenom Manhattan, Washington DC og San Francisco', 12300, 'USD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'USA', 'USA', 'Washington DC', 13800, 'USD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Uruguay', 'URY', 'Montevideo', 78900, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Venezuela', 'VEN', 'Caracas', 57600, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Amerika', 'Amerika øvrige', null, '', 90100, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Afghanistan', 'AFG', 'Kabul', 74200, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Armenia', 'ARM', 'Jerevan', 66200, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Aserbajdsjan', 'AZE', 'Baku', 65200, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Bahrain', 'BHR', 'Manama', 116500, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Bangladesh', 'BGD', 'Dhaka', 739300, 'BDT', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Brunei', 'BRN', 'Bandar Seri Begawan', 65100, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Kambodsja', 'KHM', 'Phnom Penh', 7800, 'USD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Kina', 'CHN', 'Beijing', 57500, 'CNY', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Kina', 'CHN', 'Shanghai', 58700, 'CNY', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Øst-Timor', 'TLS', 'Dili', 6500, 'USD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Georgia', 'GEO', 'Tbilisi', 51000, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Hongkong', 'HKG', 'Hong Kong', 90500, 'HKD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'India', 'IND', 'New Delhi', 402700, 'INR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Indonesia', 'IDN', 'Jakarta', 101670100, 'IDR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Iran', 'IRN', 'Teheran', 30100, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Israel', 'ISR', 'Jerusalem', 41300, 'ILS', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Japan', 'JPN', 'Tokyo', 927100, 'JPY', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Jemen', 'YEM', 'Sanaá', 74200, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Jordan', 'JOR', 'Amman', 107300, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Kasakhstan', 'KAZ', 'Astana', 57400, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Sør-Korea', 'KOR', 'Seoul', 10519700, 'KRW', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Kuwait', 'KWT', 'Kuwait by', 108400, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Laos', 'LAO', 'Vientiane', 39300, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Nord Korea', 'PRK', 'Pyongyang', 74200, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Malaysia', 'MYS', 'Kuala Lumpur', 20300, 'MYR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Maldivene', 'MDV', 'Male', 67300, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Myanmar', 'MMR', 'Yangon', 11751100, 'MMK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Oman', 'OMN', 'Muscat', 110600, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Pakistan', 'PAK', 'Islamabad', 1108800, 'PKR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Palestina', 'PSE', '', 41700, 'ILS', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Filippinene', 'PHL', 'Metro-Manila', 404200, 'PHP', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Qatar', 'QAT', 'Doha', 109300, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Saudi-Arabia', 'SAU', 'Riyadh', 124900, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Singapore', 'SGP', 'Singapore', 14600, 'SGD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Sri Lanka', 'LKA', 'Colombo', 66100, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Taiwan', 'TWN', 'Taipei', 247100, 'TWD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Thailand', 'THA', 'Bangkok', 245600, 'THB', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Tyrkia', 'TUR', 'Ankara', 236800, 'TRY', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Tyrkia', 'TUR', 'Istanbul', 232700, 'TRY', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'De forente arabiske emirater', 'ARE', 'Abu Dhabi', 118500, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Usbekistan', 'UZB', 'Tasjkent', 43900, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Vietnam', 'VNM', 'Hanoi', 139079400, 'VND', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Asia', 'Asia øvrige', null, '', 74200, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Australia/Oceania', 'Australia', 'AUS', 'Canberra', 15400, 'AUD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Australia/Oceania', 'New Zealand', 'NZL', 'Wellington', 12900, 'NZD', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Australia/Oceania', 'Papua Ny-Guinea', 'PNG', 'Port Moresby', 74800, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Australia/Oceania', 'Australia/Oceania øvrige', null, '', 84700, 'NOK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026'),
('Europa', 'Andorra', 'AND', '', 7700, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026 (som for Spania)'),
('Europa', 'Azorene', 'PRT', '', 7500, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026 (som for Portugal)'),
('Europa', 'Færøyene', 'FRO', '', 99400, 'DKK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026 (som for Danmark)'),
('Europa', 'Gibraltar', 'GIB', '', 7800, 'GBP', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026 (som for Storbritannia, utenom London)'),
('Europa', 'Grønland', 'GRL', '', 99400, 'DKK', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026 (som for Danmark)'),
('Europa', 'Guernsey (inkl. Alderney og Sark)', 'GGY', '', 7800, 'GBP', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026 (som for Storbritannia, utenom London)'),
('Europa', 'Jersey', 'JEY', '', 7800, 'GBP', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026 (som for Storbritannia, utenom London)'),
('Europa', 'Kanariøyene', 'ESP', '', 7700, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026 (som for Spania)'),
('Europa', 'Madeira', 'PRT', '', 7500, 'EUR', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026 (som for Portugal)'),
('Europa', 'Man (Isle of Man)', 'IMN', '', 7800, 'GBP', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026 (som for Storbritannia, utenom London)'),
('Europa', 'Nord-Irland', 'GBR', '', 7800, 'GBP', 'Særavtale reise utenfor Norge, vedlegg: kostsatser fra 1.1.2026 (som for Storbritannia, utenom London)');

-- What a payroll run paid for a trip, per a-melding description: the
-- snapshot the melding is rebuilt from, like payroll_expense for mileage.
create table payroll_travel (
    id          uuid primary key,
    run_id      uuid not null references payroll_run (id),
    employee_id uuid not null references employee (id),
    expense_id  uuid not null references expense (id),
    report      text not null check (report in ('kost_hotell', 'kost_pensjonat', 'kost_privat',
                                                'kost_dagreise', 'nattillegg',
                                                'kost_trekkpliktig', 'annet_trekkpliktig')),
    amount_ore  bigint not null check (amount_ore > 0),
    days        bigint not null check (days >= 0),
    created_at  timestamptz not null default now(),
    -- A claim is paid once.
    unique (expense_id, report)
);

create index payroll_travel_run_idx on payroll_travel (run_id, employee_id);

create trigger payroll_travel_append_only
    before update or delete on payroll_travel
    for each row execute function forbid_ledger_mutation();
create trigger payroll_travel_no_truncate
    before truncate on payroll_travel
    for each statement execute function forbid_ledger_mutation();

grant select, insert on payroll_travel to regnmed_app;
