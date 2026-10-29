-- En agent er en BRUKER, ikke en tilgangskategori (#101, docs/auth.md).
--
-- 0033 ga `integration_grant.access` to verdier, 'les' og 'bokforing',
-- og begrunnelsen var at det ikke skulle gå an å slippe en maskin inn i
-- tilgangsstyringen. Begrunnelsen holder; UTFALLET gjorde det ikke.
--
-- Et nytt kundeforløp står foran MIGRERINGEN — den vanskeligste
-- oppgaven i systemet, der hjelp er verdt mest — og `import_saft`
-- krever `MIGRERING_ADMIN`, som ingen maskin kunne få. Sperren var altså
-- strengest der brukeren er mest hjelpeløs. Det er ikke en
-- sikkerhetsegenskap.
--
-- Erstatningen er ikke en ny tilgangsmodell, men den som mennesker alt
-- har: rollenavnet. Innebygde roller OG selskapets egne (#60) kan
-- tildeles en integrasjon på samme måte som til et medlem, validert i
-- koden mot samme vokabular som ellers (basen kjenner ikke vokabularet,
-- så en CHECK her ville vært en annen sannhet enn tilgangsvakten).
--
-- GRENSEN SOM BESTÅR er `Rett::kan_delegeres`: en integrasjon får
-- aldri en rettighet som styrer HVEM SOM HAR TILGANG, uansett hvilken
-- rolle den bærer. Den filtreringen ligger i oppslaget (tilgang.rs), ett
-- sted, slik at den gjelder også for roller som kommer til senere — og
-- den gjelder derfor også eksisterende rader. #57/#63-grensen står urørt:
-- en maskin gjør jobben, men utvider aldri tilgang.
--
-- Tilbakekalling er uendret: `valid_to` er eksklusiv, altså umiddelbar,
-- og innholdet i en tilgang er fortsatt uforanderlig (0033s trigger).

alter table integration_grant drop constraint integration_grant_access_check;
alter table integration_grant add constraint integration_grant_access_check
    check (access <> '');
