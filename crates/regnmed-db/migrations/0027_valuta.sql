-- Flervaluta (docs/valuta.md, #44). Bokføringsvalutaen er NOK;
-- posteringslinjer kan i tillegg bære transaksjonsvalutaen — hva
-- transaksjonen lød på og kursen den ble bokført til. Feltene er
-- bevis og dekkes av hash format v4 (regnmed-core::hash).
--
-- Kursene er markedsdata, ikke selskapsdata: én global datert tabell
-- (som vat_rate/sats), matet fra Norges Banks åpne API eller manuelt —
-- kilden står alltid på raden. Append-only som alt regelverksaktig:
-- én kurs per (valuta, dato), aldri endret.

create table valutakurs (
    valuta     text not null check (valuta ~ '^[A-Z]{3}$' and valuta <> 'NOK'),
    dato       date not null,
    -- NOK per valutaenhet i mikro-NOK (11,6543 → 11654300).
    kurs_micro bigint not null check (kurs_micro > 0),
    kilde      text not null check (kilde <> ''),
    created_at timestamptz not null default now(),
    primary key (valuta, dato)
);

create trigger valutakurs_append_only
    before update or delete on valutakurs
    for each row execute function forbid_ledger_mutation();
create trigger valutakurs_no_truncate
    before truncate on valutakurs
    for each statement execute function forbid_ledger_mutation();

grant select, insert on valutakurs to regnmed_app;

-- Valutainformasjon på posteringslinjen: alle tre felt sammen, eller
-- ingen. Immutability arves fra entry-tabellens append-only-triggere.
alter table entry add column valuta text
    check (valuta is null or (valuta ~ '^[A-Z]{3}$' and valuta <> 'NOK'));
alter table entry add column valutabelop_cent bigint;
alter table entry add column kurs_micro bigint check (kurs_micro is null or kurs_micro > 0);
alter table entry add constraint entry_valuta_all_or_none check (
    (valuta is null) = (valutabelop_cent is null)
    and (valuta is null) = (kurs_micro is null)
);

-- Matching i valuta: raden husker hvor mange valuta-cent den lukket,
-- så åpen valutarest = valutabelop_cent − SUM(valuta_cent) — samme
-- beregnede disiplin som NOK-restene.
alter table reskontro_match add column valuta_cent bigint
    check (valuta_cent is null or valuta_cent > 0);

-- Dokumentvalutaen på fakturaen (visning/sporbarhet; beløpene på
-- linjene er i valutaens cent når satt).
alter table invoice add column valuta text
    check (valuta is null or (valuta ~ '^[A-Z]{3}$' and valuta <> 'NOK'));
