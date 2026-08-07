-- Flere stamdata fra Enhetsregisteret ved onboarding.
--
-- Navnet ble hentet fra registeret fra første dag; adressen og
-- registreringsstatusen kom med #81. Disse to har en NAVNGITT framtidig
-- bruker hver, og hentes nå mens vi likevel står i svaret:
--
--   naeringskode     — næringsspesifikasjonen (#11) skal oppgi NACE, og
--                      å spørre brukeren om noe registeret allerede vet
--                      er en unødvendig utfylling.
--   aksjekapital     — aksjeeierboken (#43) fører aksjer og eierandeler;
--   antall_aksjer      registerets tall er fasit å kontrollere mot, ikke
--                      en kilde å bokføre fra.
--
-- Alt tre er REDIGERBAR stamdata som alle de andre kolonnene fra 0019 —
-- ingen av dem inngår i noen hash, og ingen av dem styrer en postering.
-- Aksjekapitalen lagres i ØRE som alle andre beløp; registeret sender
-- den som JSON-tall og den parses desimalt, aldri gjennom en float.

alter table company add column naeringskode      text;
alter table company add column aksjekapital_ore  bigint;
alter table company add column antall_aksjer     bigint;

grant update (naeringskode, aksjekapital_ore, antall_aksjer) on company to regnmed_app;

comment on column company.naeringskode is
    'NACE fra Enhetsregisteret (naeringskode1). Til næringsspesifikasjonen (#11); ingen konsument ennå.';
comment on column company.aksjekapital_ore is
    'Registrert aksjekapital i øre fra Enhetsregisteret. Kontrolltall for aksjeeierboken (#43) — bokføres aldri.';
