-- Kobler den ansatte til portalbrukeren som fører timer (docs/lonn.md,
-- #46).
--
-- `time_entry` er ført av en `person` — en som logger inn i portalen.
-- `employee` er lønnsmottakeren, identifisert ved fødselsnummer fordi
-- det er slik a-meldingen rapporterer. Det er to forskjellige ting, og
-- de skal fortsette å være det: en ansatt trenger ikke portaltilgang,
-- og en portalbruker er ikke nødvendigvis ansatt.
--
-- Men skal timelønn beregnes fra timeføringen, må noen si hvem som er
-- hvem. Koblingen er derfor eksplisitt og valgfri, satt av en admin —
-- ikke gjettet ut fra navn, som ville koblet feil person til feil lønn
-- første gang to ansatte het det samme.
alter table employee
    add column person_id uuid references person (id);

-- Én portalbruker kan ikke være to ansatte i samme selskap; da ville
-- timene blitt betalt to ganger.
create unique index employee_person_idx
    on employee (company_id, person_id)
    where person_id is not null;

grant update (person_id) on employee to regnmed_app;
