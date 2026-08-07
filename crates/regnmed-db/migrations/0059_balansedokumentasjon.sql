-- Balansedokumentasjon (#88): dokumentasjon av hva en balansekonto
-- BESTÅR AV ved periodeslutt — bokføringsloven §11, jf.
-- bokføringsforskriften kap. 6, oppbevaringsplikt fem år (bfl. §13).
--
-- Vi hadde bankavstemming, åpne poster i reskontroen og
-- revisjonsrapportens kontroller, men ingen struktur for selve
-- avstemmingen. Et selskap kunne låse en periode uten at én eneste
-- balansekonto var avstemt, og revisor hadde ingen annen kilde enn
-- e-post.
--
-- Doktrinen er periodelåsens og attesteringens: INNSETTINGS-BART spor.
-- En retting er en NY RAD, og den nyeste gjelder. Ingenting oppdateres,
-- ingenting slettes — en avstemming som kunne skrives om i ettertid er
-- ikke dokumentasjon, den er en påstand.
--
-- SALDOEN LAGRES, og det er selve poenget: er kontoen bokført videre
-- etter at den ble avstemt, skal rapporten SI det. Et øyeblikksbilde
-- som stille fulgte hovedboken ville skjult nøyaktig den forskjellen
-- avstemmingen finnes for å fange.

create table balanse_dokumentasjon (
    id             uuid primary key,
    company_id     uuid not null references company (id),
    -- Kontonummeret, ikke konto-id: kontoen er permanent i kontoplanen,
    -- og nummeret er det revisor slår opp.
    konto          text not null check (konto ~ '^[0-9]{4}$'),
    -- Periodeslutt (siste dag i perioden som dokumenteres).
    periode        date not null,
    -- Bokført saldo i øre da avstemmingen ble gjort.
    saldo_ore      bigint not null,
    -- Hva saldoen består av. Fritekst med vilje: en kontoutskrift, en
    -- varetellingsliste og en lånesaldo forklares ikke i samme skjema.
    forklaring     text not null check (forklaring <> ''),
    -- Vedlegget ER dokumentasjonen når det finnes (kontoutskriften, den
    -- signerte tellelista). Innholdet er uforanderlig som
    -- bilagsvedleggene: hashen sjekkes ved nedlasting.
    vedlegg        bytea,
    vedlegg_navn   text,
    vedlegg_type   text,
    vedlegg_sha256 bytea,
    avstemt_av     uuid not null references person (id),
    avstemt_dato   date not null,
    created_at     timestamptz not null default now(),
    -- Enten et helt vedlegg eller ingen deler av et.
    check ((vedlegg is null) = (vedlegg_sha256 is null)),
    check ((vedlegg is null) = (vedlegg_navn is null)),
    check (vedlegg_sha256 is null or length(vedlegg_sha256) = 32)
);

create index balanse_dokumentasjon_oppslag
    on balanse_dokumentasjon (company_id, periode, konto, created_at desc);

create trigger balanse_dokumentasjon_append_only
    before update or delete on balanse_dokumentasjon
    for each row execute function forbid_ledger_mutation();
create trigger balanse_dokumentasjon_no_truncate
    before truncate on balanse_dokumentasjon
    for each statement execute function forbid_ledger_mutation();

grant select, insert on balanse_dokumentasjon to regnmed_app;
