-- Nødprosedyren har et navn (#57, docs/auth.md §8).
--
-- Det finnes ingen plattformadministrator, og støtteveien er at kunden
-- selv gir tilgang. Det ene tilfellet det svaret ikke dekker, er
-- selskapet hvis eneste administrator er borte for godt: siste admin
-- kan ikke fjerne seg selv (0037), men et dødsfall eller en brå
-- avslutning spør ikke databasen først.
--
-- Prosedyren (dokumentert i sin helhet i docs/auth.md) er en manuell
-- DB-operasjon med skriftlig samtykke — og sporet den etterlater skal
-- HETE det den er. Uten en egen kilde måtte innslaget ha utgitt seg
-- for å være en vanlig admin-handling, og da ville tilgangsloggen
-- løyet om akkurat det innslaget den finnes for å fange.
alter table company_member_change
    drop constraint company_member_change_kilde_check;
alter table company_member_change
    add constraint company_member_change_kilde_check
    check (kilde in ('admin', 'invitasjon', 'onboarding', 'nodprosedyre'));

-- Referansen til samtykket, rett i sporet. Valgfri ellers — men et
-- nødinnslag UTEN referanse er nettopp det uattribuerte inngrepet
-- prosedyren skal umuliggjøre, så der er den påbudt.
alter table company_member_change
    add column notat text;
alter table company_member_change
    add constraint company_member_change_nodprosedyre_notat_check
    check (kilde <> 'nodprosedyre' or (notat is not null and notat <> ''));
