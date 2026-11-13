-- Expense drafts, on-behalf entry, dimensions and the trip record
-- (docs/utlegg.md).
--
-- A claim now starts as a DRAFT ('utkast'): a receipt photographed on the
-- way out of the taxi is saved at once and finished later. Nothing about a
-- draft is evidence yet -- every field may change, the receipt may be
-- replaced (rotated, cropped) -- and the innboks discipline begins where it
-- always did: at submission ('innsendt'), after which the content is
-- immutable. A draft is never deleted either; it is discarded
-- ('forkastet'), one way.
--
-- Also new:
-- - registered_by_person: who ENTERED the claim, when someone files it on
--   another person's behalf. person_id stays whose money it is.
-- - department / project (dimension CODES, which are permanent) and
--   billable, posted onto the cost lines at approval.
-- - Trip documentation: from/to, round trip, vehicle, passengers (names --
--   the passenger supplement is for colleagues on the assignment, and the
--   documentation must say who). The passenger rate is stored at
--   submission like the km rate: the row is evidence.
-- - trip_expense_id: tolls, rush-hour fees, ferries and parking are
--   ordinary receipt claims tied to the trip they belong to.

alter table expense drop constraint expense_status_check;
alter table expense add constraint expense_status_check
    check (status in ('utkast', 'innsendt', 'godkjent', 'avvist', 'utbetalt', 'forkastet'));

-- A draft may be incomplete; a submitted claim may not.
alter table expense alter column dato drop not null;
alter table expense alter column belop_ore drop not null;
alter table expense drop constraint expense_belop_ore_check;
alter table expense drop constraint expense_beskrivelse_check;
alter table expense drop constraint expense_check1;
alter table expense drop constraint expense_receipt_check;
alter table expense add constraint expense_complete_check
    check (status in ('utkast', 'forkastet')
           or (dato is not null and belop_ore > 0 and beskrivelse <> ''));
alter table expense add constraint expense_run_check
    check (kind <> 'kjoring' or status in ('utkast', 'forkastet')
           or (km is not null and sats_ore_per_km is not null
               and trekkfri_ore is not null and trekkpliktig_ore is not null));
alter table expense add constraint expense_receipt_check
    check (kind <> 'utlegg' or status in ('utkast', 'forkastet')
           or (receipt_sha256 is not null
               and receipt_filename is not null and receipt_content_type is not null));

alter table expense
    add column registered_by_person uuid references person (id),
    add column department text,
    add column project text,
    add column billable boolean not null default false,
    add column route_from text,
    add column route_to text,
    add column round_trip boolean not null default false,
    add column vehicle text check (vehicle in ('bil', 'bil_utslippsfri', 'motorsykkel',
                                               'moped', 'snoscooter', 'bat')),
    add column passengers text[] not null default '{}',
    add column passenger_rate_ore_per_km bigint,
    add column trip_expense_id uuid references expense (id),
    add constraint expense_trip_check check (trip_expense_id is null or kind = 'utlegg'),
    add constraint expense_vehicle_kind_check check (kind = 'kjoring' or vehicle is null);

create index expense_trip_idx on expense (trip_expense_id) where trip_expense_id is not null;

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
       or new.trip_expense_id is distinct from old.trip_expense_id then
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

-- The trigger decides what may change and when; the grant only has to
-- let a draft be written at all.
grant update (dato, beskrivelse, belop_ore, receipt_filename, receipt_content_type,
              receipt_content, receipt_sha256, km, sats_ore_per_km, trekkfri_ore,
              trekkpliktig_ore, department, project, billable, route_from, route_to,
              round_trip, vehicle, passengers, passenger_rate_ore_per_km, trip_expense_id)
    on expense to regnmed_app;

-- Kilometre rates for the other vehicles and the per-km supplement for
-- each colleague on the assignment. Statens særavtale reise i Norge
-- 2026-2027 §§ 2-3 (gjelder 1.1.2026-31.12.2027), and the trekkfri side
-- from Skatteetaten's satser for bilgodtgjørelse 2026 -- both fetched
-- 2026-11-13. Same values on both sides for all of these.
insert into sats (domene, valid_from, verdi, enhet, kilde) values
('km_godtgjorelse_motorsykkel',          '2026-01-01',  295, 'ore_per_km', 'Særavtale reise i Norge 2026-2027 § 3: motorsykkel over 125 ccm'),
('km_godtgjorelse_motorsykkel_trekkfri', '2026-01-01',  295, 'ore_per_km', 'Skatteetaten, satser bilgodtgjørelse 2026: tung motorsykkel'),
('km_godtgjorelse_moped',                '2026-01-01',  200, 'ore_per_km', 'Særavtale reise i Norge 2026-2027 § 3: moped, motorsykkel til og med 125 ccm, andre motoriserte'),
('km_godtgjorelse_moped_trekkfri',       '2026-01-01',  200, 'ore_per_km', 'Skatteetaten, satser bilgodtgjørelse 2026: moped/lett motorsykkel'),
('km_godtgjorelse_snoscooter',           '2026-01-01', 1000, 'ore_per_km', 'Særavtale reise i Norge 2026-2027 § 3: snøscooter og ATV'),
('km_godtgjorelse_snoscooter_trekkfri',  '2026-01-01', 1000, 'ore_per_km', 'Skatteetaten, satser bilgodtgjørelse 2026: snøscooter og ATV'),
('km_godtgjorelse_bat',                  '2026-01-01',  750, 'ore_per_km', 'Særavtale reise i Norge 2026-2027 § 3: båt med motor'),
('km_godtgjorelse_bat_trekkfri',         '2026-01-01',  750, 'ore_per_km', 'Skatteetaten, satser bilgodtgjørelse 2026: motorbåt'),
('km_passasjertillegg',                  '2026-01-01',  100, 'ore_per_km', 'Særavtale reise i Norge 2026-2027 §§ 2-3: per passasjer'),
('km_passasjertillegg_trekkfri',         '2026-01-01',  100, 'ore_per_km', 'Skatteetaten, satser bilgodtgjørelse 2026: passasjertillegg');
