-- Lønn, første del (docs/lonn.md, #46): ansattregister og lønnskjøring.
--
-- Lønn er den mest regelstyrte delen av et regnskapssystem, og denne
-- migrasjonen tar bare det som kan gjøres riktig i dag: fastlønn og
-- timelønn, prosenttrekk fra skattekortet, arbeidsgiveravgift per sone,
-- og feriepengeavsetning. Tabelltrekk, a-melding, sykepengerefusjon og
-- naturalytelser står igjen — se docs/lonn.md for hva som mangler og
-- hvorfor, slik at ingen tror dette er en komplett lønnsmodul.
--
-- En lønnskjøring er ETT bilag. Linjene lagres for lønnsslippen og for
-- a-meldingen senere, men tallene i hovedboken er bilaget — ingen
-- parallell sannhet.

create table employee (
    id             uuid primary key,
    company_id     uuid not null references company (id),
    -- Identiteten er permanent: den er det a-meldingen rapporterer
    -- under, og et bytte er en ny ansatt, ikke en redigering.
    fodselsnummer  text not null check (fodselsnummer ~ '^[0-9]{11}$'),
    navn           text not null check (navn <> ''),
    stilling       text,
    ansatt_fra     date not null,
    ansatt_til     date,
    check (ansatt_til is null or ansatt_til >= ansatt_fra),
    -- Fastlønn per måned. Timelønn og stillingsprosent er de to andre
    -- vanlige formene; en kjøring bruker den som er satt.
    manedslonn_ore bigint check (manedslonn_ore is null or manedslonn_ore >= 0),
    timelonn_ore   bigint check (timelonn_ore is null or timelonn_ore >= 0),
    -- Skattekortet. `tabell` lagres, men en kjøring med tabelltrekk
    -- nektes i beregningen — vi tilnærmer ikke Skatteetatens tabeller.
    trekk_type     text not null default 'prosent'
                     check (trekk_type in ('prosent', 'tabell', 'ingen')),
    trekk_prosent_bp integer check (trekk_prosent_bp between 0 and 10000),
    trekk_tabell   integer,
    check (case trekk_type
               when 'prosent' then trekk_prosent_bp is not null
               when 'tabell' then trekk_tabell is not null
               else true
           end),
    -- Feriepengesatsen i basispunkter: 1020 etter ferieloven §10, 1250
    -- fra året den ansatte fyller 60 (§10 nr. 3), 1200/1430 på tariff
    -- med fem uker. Lagret per ansatt fordi det er et faktum om
    -- arbeidsforholdet, ikke en systeminnstilling.
    feriepenger_bp integer not null default 1020
                     check (feriepenger_bp between 0 and 5000),
    bank_account   text,
    note           text,
    created_by     text not null check (created_by <> ''),
    created_at     timestamptz not null default now(),
    -- Samme person kan ikke stå to ganger i samme ansattregister.
    unique (company_id, fodselsnummer)
);

create index employee_company_idx on employee (company_id, navn);

create function employee_guard() returns trigger
language plpgsql as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'ansatte slettes ikke — sett ansatt_til i stedet';
    end if;
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.fodselsnummer is distinct from old.fodselsnummer
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'den ansattes identitet er uforanderlig (ansatt %)', old.id;
    end if;
    return new;
end;
$$;

create trigger employee_guard_trg
    before update or delete on employee
    for each row execute function employee_guard();

grant select, insert on employee to regnmed_app;
grant update (navn, stilling, ansatt_til, manedslonn_ore, timelonn_ore,
              trekk_type, trekk_prosent_bp, trekk_tabell, feriepenger_bp,
              bank_account, note)
    on employee to regnmed_app;

-- Lønnskjøring: én per selskap per måned, innsettings-bar som bilag.
create table payroll_run (
    id           uuid primary key,
    company_id   uuid not null references company (id),
    ar           integer not null check (ar between 1900 and 2999),
    maned        integer not null check (maned between 1 and 12),
    -- Utbetalingsdato styrer hvilke satser som gjelder.
    utbetalt_dato date not null,
    sone         text not null check (sone <> ''),
    -- Summene, lagret slik de ble bokført.
    brutto_ore              bigint not null,
    feriepenger_utbetalt_ore bigint not null default 0,
    forskuddstrekk_ore      bigint not null,
    netto_ore               bigint not null,
    feriepengeavsetning_ore bigint not null,
    aga_ore                 bigint not null,
    aga_feriepenger_ore     bigint not null,
    voucher_id   uuid not null references voucher (id),
    note         text,
    created_by   text not null check (created_by <> ''),
    created_at   timestamptz not null default now(),
    -- Samme måned kan ikke kjøres to ganger; en korreksjon er et
    -- reverserende bilag og en ny kjøring, som ellers i hovedboken.
    unique (company_id, ar, maned)
);

create index payroll_run_company_idx on payroll_run (company_id, ar desc, maned desc);

-- Én linje per ansatt, grunnlaget for lønnsslipp og senere a-melding.
create table payroll_line (
    id            uuid primary key,
    run_id        uuid not null references payroll_run (id),
    employee_id   uuid not null references employee (id),
    brutto_ore    bigint not null,
    feriepenger_ore bigint not null default 0,
    trekkgrunnlag_ore bigint not null,
    forskuddstrekk_ore bigint not null,
    netto_ore     bigint not null,
    feriepengeavsetning_ore bigint not null,
    aga_ore       bigint not null,
    aga_feriepenger_ore bigint not null,
    halv_trekk    boolean not null default false,
    unique (run_id, employee_id)
);

create index payroll_line_run_idx on payroll_line (run_id);

create function payroll_guard() returns trigger
language plpgsql as $$
begin
    raise exception 'lønnskjøringer er innsettings-bare — rett med et reverserende bilag og en ny kjøring';
end;
$$;

create trigger payroll_run_guard_trg
    before update or delete on payroll_run
    for each row execute function payroll_guard();
create trigger payroll_line_guard_trg
    before update or delete on payroll_line
    for each row execute function payroll_guard();

grant select, insert on payroll_run to regnmed_app;
grant select, insert on payroll_line to regnmed_app;

-- Satsene som data, med kilde per rad (docs/regelverk.md).
--
-- Arbeidsgiveravgift: Skattedirektoratets melding «Arbeidsgiveravgift
-- til folketrygden for 2026», hentet 2026-07-27. Meldingen sier selv at
-- verken soneinndeling eller satser er endret fra 2025 til 2026, så
-- periodene starter 2025-01-01 — den tidligste datoen vi har VERIFISERT,
-- ikke en gjettet historikk.
--
-- Merk: den ekstra arbeidsgiveravgiften på 5 % over 750 000 kroner ble
-- FJERNET fra 2025. Den finnes ikke her fordi den ikke finnes.
--
-- Sone Ia har ingen rad. Satsen der gjelder bare til fribeløpet er
-- brukt opp, og det kan ikke leses ut av én sats — se docs/lonn.md.
insert into sats (domene, valid_from, verdi, enhet, kilde) values
    ('aga_sone_i',   date '2025-01-01', 1410, 'bp',
     'Skattedirektoratets melding, arbeidsgiveravgift til folketrygden for 2026 (uendret fra 2025)'),
    ('aga_sone_ii',  date '2025-01-01', 1060, 'bp',
     'Skattedirektoratets melding, arbeidsgiveravgift til folketrygden for 2026 (uendret fra 2025)'),
    ('aga_sone_iii', date '2025-01-01',  640, 'bp',
     'Skattedirektoratets melding, arbeidsgiveravgift til folketrygden for 2026 (uendret fra 2025)'),
    ('aga_sone_iv',  date '2025-01-01',  510, 'bp',
     'Skattedirektoratets melding, arbeidsgiveravgift til folketrygden for 2026 (uendret fra 2025)'),
    ('aga_sone_iva', date '2025-01-01',  790, 'bp',
     'Skattedirektoratets melding, arbeidsgiveravgift til folketrygden for 2026 (uendret fra 2025)'),
    ('aga_sone_v',   date '2025-01-01',    0, 'bp',
     'Skattedirektoratets melding, arbeidsgiveravgift til folketrygden for 2026 (uendret fra 2025) — nullsats er en sats, ikke en manglende verdi');

-- Feriepenger etter ferieloven §10. Lovfestet og endres sjelden; den
-- tidligste verifiserte datoen er da satsene sist ble endret i loven.
insert into sats (domene, valid_from, verdi, enhet, kilde) values
    ('feriepenger_lovens_minimum', date '2025-01-01', 1020, 'bp',
     'ferieloven §10 nr. 2 — 10,2 % av feriepengegrunnlaget'),
    ('feriepenger_over_60',        date '2025-01-01', 1250, 'bp',
     'ferieloven §10 nr. 3 — 10,2 % + 2,3 prosentpoeng fra året arbeidstakeren fyller 60');
