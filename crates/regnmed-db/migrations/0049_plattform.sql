-- Plattformroller: systemadmin og support (docs/auth.md §8).
--
-- Fram til nå fantes ingen tilgangsvei som krysset selskapsgrenser, og
-- det var en avgjørelse (#57). Denne migrasjonen bygger den bevisst
-- avgrensede unntaksveien §8 selv stilte kravene til: en plattformrolle
-- når ADMINISTRATIVE STAMDATA (personer, medlemskap, kunderegistre) —
-- aldri hovedboken. Svaret på revisorens spørsmål «hvem hos leverandøren
-- kan lese klientens regnskap?» er fortsatt *ingen*, og tilgangsvakten
-- for selskapsdata (tilgang::krev) er uendret.
--
-- Kravene fra #57 håndheves her, ikke i prosa:
--   * TIDSBEGRENSET: valid_to er obligatorisk — en plattformrolle uten
--     utløpsdato kan ikke skrives inn.
--   * LOGGET: hvert eneste kall mot /platform-endepunktene får en rad i
--     platform_access_log (innsettings-bar, én vakt i API-et som ingen
--     endepunkter kan gå utenom).
--   * VARSLET: loggen leses av selskapets egne administratorer gjennom
--     /companies/{id}/platform-access — tilgangen er synlig for den den
--     gjelder, ikke bare for plattformen.

create table platform_member (
    id uuid primary key default gen_random_uuid(),
    person_id uuid not null references person (id),
    rolle text not null check (rolle in ('systemadmin', 'support')),
    valid_from date not null default current_date,
    -- Exclusive, like engagement: active while valid_from <= today < valid_to.
    -- NOT NULL is the point — #57 demands the role be time-limited.
    valid_to date not null,
    -- Why this person holds the role; a support case or an appointment
    -- reference. Mandatory for the same reason the nodprosedyre demands
    -- its consent reference: the record exists to answer "why".
    notat text not null check (notat <> ''),
    -- NULL means granted from the CLI (bootstrap — the first systemadmin
    -- cannot be granted through an API only systemadmins may call).
    granted_by uuid references person (id),
    created_at timestamptz not null default now(),
    check (valid_to >= valid_from)
);

create index platform_member_person_idx on platform_member (person_id, valid_to desc);

-- Insert + end only: ending a membership is an update of valid_to alone
-- (column grant), nothing is ever deleted. Same discipline as engagement
-- and abonnement.
create trigger platform_member_no_delete
    before delete on platform_member
    for each row execute function forbid_ledger_mutation();
create trigger platform_member_no_truncate
    before truncate on platform_member
    for each statement execute function forbid_ledger_mutation();

grant select, insert on platform_member to regnmed_app;
grant update (valid_to) on platform_member to regnmed_app;

-- Every call a platform role makes, in one insert-only trail. company_id
-- and firm_id are set when the call concerns one, so the affected party
-- can read exactly the rows that concern them.
create table platform_access_log (
    id uuid primary key default gen_random_uuid(),
    person_id uuid not null references person (id),
    rolle text not null check (rolle in ('systemadmin', 'support')),
    method text not null check (method <> ''),
    path text not null check (path <> ''),
    company_id uuid references company (id),
    firm_id uuid references firm (id),
    created_at timestamptz not null default now()
);

create index platform_access_log_company_idx
    on platform_access_log (company_id, created_at desc);
create index platform_access_log_firm_idx
    on platform_access_log (firm_id, created_at desc);
create index platform_access_log_idx on platform_access_log (created_at desc);

create trigger platform_access_log_append_only
    before update or delete on platform_access_log
    for each row execute function forbid_ledger_mutation();
create trigger platform_access_log_no_truncate
    before truncate on platform_access_log
    for each statement execute function forbid_ledger_mutation();

grant select, insert on platform_access_log to regnmed_app;

-- Membership changes made through the platform path name their source,
-- so the company's own tilgangshistorikk shows who let a person in: a
-- 'plattform' row always carries utfort_av (the platform person).
alter table company_member_change
    drop constraint company_member_change_kilde_check;
alter table company_member_change
    add constraint company_member_change_kilde_check
    check (kilde in ('admin', 'invitasjon', 'onboarding', 'nodprosedyre', 'plattform'));

alter table firm_member_change
    drop constraint firm_member_change_kilde_check;
alter table firm_member_change
    add constraint firm_member_change_kilde_check
    check (kilde in ('admin', 'invitasjon', 'registrering', 'plattform'));
