-- Ansattrollen (#54, docs/auth.md).
--
-- Rollene var 'admin', 'bokforing' og 'les' — en rangstige. En vanlig
-- ansatt som skal føre sine egne timer, sende inn sitt eget utlegg og
-- se sin egen lønnsslipp passet ingen steder: laveste trinn ('les')
-- kunne ikke skrive noe som helst, og det trinnet som kunne skrive
-- ('bokforing') ga full skrivetilgang til hovedboken.
--
-- 'ansatt' er ikke et fjerde trinn. Den er et lite, positivt avgrenset
-- sett rettigheter (ANSATT_BUNT i regnmed-api::tilgang) som får SKRIVE
-- noen få egne ting og LESE nesten ingenting — en form stigen ikke
-- kunne uttrykke.
--
-- Ingen har rollen fra før, så ingen tilgang endres av denne
-- migrasjonen.
alter table company_member drop constraint company_member_role_check;
alter table company_member add constraint company_member_role_check
    check (role in ('admin', 'bokforing', 'les', 'ansatt'));

alter table company_invitation drop constraint company_invitation_role_check;
alter table company_invitation add constraint company_invitation_role_check
    check (role in ('admin', 'bokforing', 'les', 'ansatt'));
