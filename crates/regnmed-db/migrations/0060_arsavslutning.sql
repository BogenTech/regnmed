-- Årsavslutning (#84): resultatdisponering og skattekostnad.
--
-- Fram til nå ble et regnskapsår ALDRI avsluttet. Det var et bevisst og
-- elegant valg så langt det rakk — udisponert resultat utledes av
-- klasse 3–8-summen ved hver lesning, og balansen går i null uansett.
-- Men rskl. §6-2 krever at balansen skiller innskutt og opptjent
-- egenkapital, asl. §8-1 regner utbyttegrunnlaget ut fra nettopp den
-- fordelingen, og utbyttevedtaket vårt debiterte 2050 mot en saldo
-- systemet aldri krediterte.
--
-- Avslutningen er ET ORDINÆRT BILAG, ikke nytt maskineri: debet 8800
-- (resultatdisponering) mot kredit 2050, i samme append-only kjede som
-- alt annet, reverserbart som alt annet.
--
-- 8800 er ikke vårt valg av konto: Skatteetatens
-- næringsspesifikasjons-kodeliste gir den sin EGEN grupperingskategori,
-- `resultatDisponeringForSAF-T` («Disponering av årets overskudd/
-- dekning av årets underskudd»), atskilt fra alle resultatlinjene.
-- Fordi den ligger i klasse 8, faller `udisponert_resultat_ore`
-- automatisk til null for det avsluttede året — hovedboken bærer det
-- selv, og ingen «dette året er lukket»-tilstand må holdes synkronisert.
--
-- REKKEFØLGEN, og et avvik fra sakens ordlyd som er verdt å lese:
-- saken ba om at årsavslutning FORUTSETTER at året er periodelåst. Det
-- er umulig slik låsen virker — bilaget dateres 31.12, altså inne i
-- perioden som da ville vært låst, og `forbid_locked_period_posting`
-- (migrasjon 0011) håndhever det i databasen uavhengig av all
-- applikasjonskode. Å svekke den triggeren for årsavslutningens skyld
-- ville vært å pælme den ene garantien for å få den andre.
--
-- Løsningen gir samme vern med omvendt rekkefølge: avslutningen SETTER
-- låsen selv, i samme transaksjon som bilaget. Etterpå er året både
-- disponert og stengt for nye posteringer, som er det saken ville
-- oppnå. Låsen er insert-only med eget spor (0011), så dette er en
-- registrert handling, ikke en stille bivirkning.

create table arsavslutning (
    id                    uuid primary key,
    company_id            uuid not null references company (id),
    ar                    integer not null,
    -- Bilaget som disponerte resultatet. Avslutningen er ikke en
    -- tilstand ved siden av hovedboken; den ER bilaget, og raden her
    -- peker på det.
    voucher_id            uuid not null references voucher (id),
    -- Tallene slik de var da året ble avsluttet. Lagret av samme grunn
    -- som balansedokumentasjonens saldo (0059): blir hovedboken endret
    -- etterpå, skal forskjellen kunne SES, ikke forsvinne.
    resultat_for_skatt_ore bigint not null,
    skattekostnad_ore      bigint not null check (skattekostnad_ore >= 0),
    disponert_ore          bigint not null,
    created_by            text not null check (created_by <> ''),
    created_at            timestamptz not null default now(),
    -- Et år kan ikke disponeres to ganger. Samme vern som
    -- avskrivningene og abonnementsfaktureringen bruker.
    unique (company_id, ar)
);

create trigger arsavslutning_append_only
    before update or delete on arsavslutning
    for each row execute function forbid_ledger_mutation();
create trigger arsavslutning_no_truncate
    before truncate on arsavslutning
    for each statement execute function forbid_ledger_mutation();

grant select, insert on arsavslutning to regnmed_app;
