-- Selskapets registreringsstatus som lagret, datert stamdata (#81).
--
-- Bokføringsforskriften §5-1-2 knytter «MVA» etter organisasjonsnummeret
-- til REGISTRERING i Merverdiavgiftsregisteret, og påtegningen
-- «Foretaksregisteret» til registrering DER. Begge ble utledet av
-- dokumentet i stedet:
--
--   selger_mva_registrert: computed.vat_ore != 0
--   selger_foretaksregistrert: matches!(orgform, "AS" | "ASA")
--
-- Den første gjør at en registrert selger som fakturerer eksport,
-- fritatt eller unntatt omsetning får faktura UTEN «MVA» — mangelfullt
-- salgsdokument. Den andre utelater ANS/DA, NUF og næringsdrivende ENK,
-- som også er registreringspliktige.
--
-- Radene er DATERTE og innsettings-bare, etter mønsteret fra sats,
-- vat_rate og mva_terminordning: en virksomhet blir mva-registrert på en
-- BESTEMT DATO, og fakturaer før den datoen skal ikke ha påtegningen.
-- Det er ikke teoretisk her: tilbud og ordrebekreftelser rendres ON
-- DEMAND (salgsdokument.rs), så uten datering ville et gammelt tilbud
-- blitt rendret på nytt med dagens status.
--
-- Oppslag = nyeste rad med valid_from <= dokumentets dato.

create table company_registrering (
    id                 uuid primary key,
    company_id         uuid not null references company (id),
    valid_from         date not null,
    mva_registrert     boolean not null,
    foretaksregistrert boolean not null,
    -- brreg: hentet fra Enhetsregisteret. manuell: satt av admin.
    -- migrert: utledet av den gamle logikken da feltet ble innført, og
    -- derfor en ANTAKELSE som bør bekreftes.
    kilde              text not null check (kilde in ('brreg', 'manuell', 'migrert')),
    notat              text,
    recorded_by        text not null check (recorded_by <> ''),
    recorded_at        timestamptz not null default now(),
    unique (company_id, valid_from)
);

create index company_registrering_lookup on company_registrering (company_id, valid_from desc);

grant select, insert on company_registrering to regnmed_app;

-- Backfill. Vi dikter ikke opp nye fakta: den gamle utledningen løftes
-- fra PER DOKUMENT til PER SELSKAP, som er nettopp rettingen — en
-- selger som ÉN GANG har fakturert med utgående mva er registrert, og
-- da skal også eksportfakturaen hans bære «MVA». Raden er merket
-- 'migrert' og skal bekreftes mot Enhetsregisteret.
--
-- valid_from er satt langt tilbake fordi raden dekker all historikk
-- fram til noen registrerer en observasjon med ekte dato.
insert into company_registrering
    (id, company_id, valid_from, mva_registrert, foretaksregistrert, kilde, notat, recorded_by)
select
    gen_random_uuid(),
    c.id,
    date '1900-01-01',
    exists (
        select 1 from invoice i
        join invoice_line l on l.invoice_id = i.id
        where i.company_id = c.id and l.vat_ore <> 0
    ),
    coalesce(c.orgform in ('AS', 'ASA'), false),
    'migrert',
    'Utledet av tidligere logikk ved innføring av lagret registreringsstatus (#81). Bekreft mot Enhetsregisteret.',
    'migration 0056'
from company c;
