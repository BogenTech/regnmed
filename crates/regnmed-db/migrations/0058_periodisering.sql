-- Periodisering (#87): fordeling av kostnad og inntekt over månedene de
-- hører hjemme i — rskl. §4-1 nr. 2 og 3, opptjenings- og
-- sammenstillingsprinsippet.
--
-- Mønsteret er avskrivningenes (0025) og de repeterende fakturaenes
-- (0021): en redigerbar plan, en innsettings-bar kjørelogg, og en delvis
-- unik indeks som gjør en (plan, måned) umulig å føre to ganger.
--
-- HVA SOM PERIODISERES: nettobeløpet, aldri merverdiavgiften.
-- Tidfestingen av avgiften følger salgsdokumentet (mval. §15-9) — en
-- husleie betalt for et helt år er fradragsberettiget i sin helhet i
-- terminen fakturaen hører hjemme i, uansett hvordan kostnaden fordeles
-- i resultatet. Derfor bærer planen ingen mva-kode: den finnes på
-- KILDEBILAGET, som er ført på vanlig måte med hele avgiften.
--
-- RETNINGEN ligger i fortegnet, som i resten av hovedboken: en
-- forskuddsbetalt KOSTNAD har positivt totalbeløp (debet resultatkonto,
-- kredit balansekonto hver måned), en forskuddsbetalt INNTEKT negativt.
-- Vi lagrer ikke en «type» ved siden av fortegnet — to felter som kan
-- motsi hverandre er en feilkilde, ikke en opplysning.

create table periodisering (
    id             uuid primary key,
    company_id     uuid not null references company (id),
    -- The bilag the amount came from. Not a foreign key requirement:
    -- an opening balance or an import may be the source, and the plan
    -- must still be expressible. When set, the revisor can walk back.
    kilde_voucher  uuid references voucher (id),
    beskrivelse    text not null check (beskrivelse <> ''),
    -- Resultatkontoen beløpet skal fordeles PÅ, og balansekontoen det
    -- ligger parkert på i mellomtiden (1700 forskuddsbetalt kostnad,
    -- 2900 forskuddsbetalt inntekt). Begge oppgis av den som oppretter
    -- planen — vi gjetter aldri en konto.
    resultatkonto  text not null check (resultatkonto <> ''),
    balansekonto   text not null check (balansekonto <> ''),
    -- Nettobeløp i øre, fortegn som i hovedboken. Aldri null-beløp: en
    -- plan uten beløp er ingen plan.
    total_ore      bigint not null check (total_ore <> 0),
    -- Første og siste måned, lagret som månedens første dag.
    fra_maned      date not null,
    til_maned      date not null,
    avdeling_id    uuid references dimension (id),
    prosjekt_id    uuid references dimension (id),
    notat          text,
    -- Enveis: en plan stoppes, den slettes aldri. Måneder som alt er
    -- ført står; de gjenstående føres ikke.
    stoppet_dato   date,
    created_by     text not null check (created_by <> ''),
    created_at     timestamptz not null default now(),
    check (til_maned >= fra_maned),
    check (date_trunc('month', fra_maned) = fra_maned),
    check (date_trunc('month', til_maned) = til_maned)
);

create index periodisering_company on periodisering (company_id, fra_maned);

-- Planen er redigerbar til første kjøring — deretter er den historikk,
-- fordi bilagene som alt er ført viser til den. Håndhevet i triggeren
-- under, ikke bare i koden: en plan som endres etter at halve beløpet er
-- bokført ville gjort at delene ikke lenger summerer til totalen.
create function periodisering_guard() returns trigger
    language plpgsql as $$
begin
    if tg_op = 'DELETE' or tg_op = 'TRUNCATE' then
        raise exception 'periodisering slettes aldri — stopp planen i stedet';
    end if;
    if exists (select 1 from periodisering_run r
               where r.periodisering_id = old.id and r.voucher_id is not null) then
        -- Etter første førte måned kan BARE stoppingen settes.
        if row(new.company_id, new.kilde_voucher, new.beskrivelse, new.resultatkonto,
               new.balansekonto, new.total_ore, new.fra_maned, new.til_maned,
               new.avdeling_id, new.prosjekt_id, new.created_by)
           is distinct from
           row(old.company_id, old.kilde_voucher, old.beskrivelse, old.resultatkonto,
               old.balansekonto, old.total_ore, old.fra_maned, old.til_maned,
               old.avdeling_id, old.prosjekt_id, old.created_by) then
            raise exception 'periodiseringen er påbegynt og kan ikke endres — stopp den og opprett en ny';
        end if;
    end if;
    return new;
end $$;

create trigger periodisering_frozen_once_started
    before update or delete on periodisering
    for each row execute function periodisering_guard();
create trigger periodisering_no_truncate
    before truncate on periodisering
    for each statement execute function forbid_ledger_mutation();

grant select, insert on periodisering to regnmed_app;
grant update (beskrivelse, resultatkonto, balansekonto, total_ore, fra_maned,
              til_maned, avdeling_id, prosjekt_id, notat, stoppet_dato)
    on periodisering to regnmed_app;

-- Kjøreloggen: én rad per (plan, måned), enten et ført bilag eller en
-- logget feil — nøyaktig som asset_depreciation.
create table periodisering_run (
    id               uuid primary key,
    periodisering_id uuid not null references periodisering (id),
    -- Månedens første dag; bilaget dateres månedens siste.
    period           date not null,
    belop_ore        bigint not null,
    voucher_id       uuid references voucher (id),
    detail           text,
    created_at       timestamptz not null default now(),
    check ((voucher_id is null) <> (detail is null)),
    check (date_trunc('month', period) = period)
);

create unique index periodisering_run_once
    on periodisering_run (periodisering_id, period) where voucher_id is not null;

create trigger periodisering_run_append_only
    before update or delete on periodisering_run
    for each row execute function forbid_ledger_mutation();
create trigger periodisering_run_no_truncate
    before truncate on periodisering_run
    for each statement execute function forbid_ledger_mutation();

grant select, insert on periodisering_run to regnmed_app;
