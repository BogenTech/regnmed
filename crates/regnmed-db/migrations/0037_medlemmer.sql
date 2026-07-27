-- Medlemsadministrasjon (#53, docs/auth.md).
--
-- Fram til nå kunne ingen gi noen tilgang til et selskap. De to veiene
-- inn var å opprette selskapet selv (og bli admin) eller å få et
-- oppdrag fra et byrå. Et selskap kunne altså ikke ta inn sin egen
-- interne regnskapsfører eller en ansatt.
--
-- To ting legges til: en invitasjon, og et spor over hvem som ga hvem
-- tilgang.

-- En invitasjon er stilet til en E-POSTADRESSE, ikke til en person.
--
-- Det er ikke en omvei: `person` opprettes først når noen logger inn
-- (just-in-time fra tokenets sub), så en som aldri har brukt regnmed
-- finnes ikke å slå opp. Og det er dessuten det personvernvennlige
-- valget — et oppslag «finnes denne e-posten hos dere» ville fortalt en
-- hvilken som helst selskapsadmin hvem som er bruker på plattformen.
-- Invitasjonen svarer likt uansett, og blir til et medlemskap når
-- adressen logger inn.
create table company_invitation (
    id          uuid primary key,
    company_id  uuid not null references company (id),
    -- Normalisert (trimmet og små bokstaver) av applikasjonen.
    epost       text not null check (epost <> ''),
    role        text not null check (role in ('admin', 'bokforing', 'les')),
    invited_by  uuid not null references person (id),
    created_at  timestamptz not null default now(),
    accepted_at timestamptz,
    accepted_by uuid references person (id),
    revoked_at  timestamptz,
    revoked_by  uuid references person (id)
);

-- Én åpen invitasjon per selskap og adresse. Brukte og tilbakekalte
-- invitasjoner blir liggende — de er historikken over hvem som ble
-- sluppet inn.
create unique index company_invitation_open_uq
    on company_invitation (company_id, epost)
    where accepted_at is null and revoked_at is null;

create index company_invitation_epost_idx
    on company_invitation (epost)
    where accepted_at is null and revoked_at is null;

-- Sporet over tilgangsendringer. `company_member` bærer bare nåtilstand
-- (rolle og aktiv), så uten dette finnes det ingen måte å svare på
-- «hvem ga denne personen bokføringstilgang, og når» — et spørsmål en
-- revisor stiller.
create table company_member_change (
    id         uuid primary key,
    company_id uuid not null references company (id),
    person_id  uuid not null references person (id),
    endring    text not null check (
                   endring in ('lagt_til', 'rolle_endret', 'deaktivert', 'reaktivert')),
    fra_rolle  text,
    til_rolle  text,
    -- Hvem som utførte endringen. NULL når den skjedde fordi personen
    -- selv løste inn en invitasjon — da står invitasjonens avsender i
    -- company_invitation.invited_by.
    utfort_av  uuid references person (id),
    kilde      text not null check (kilde in ('admin', 'invitasjon', 'onboarding')),
    created_at timestamptz not null default now()
);

create index company_member_change_idx
    on company_member_change (company_id, created_at desc);

-- Sporet er innsettings-bart, som resten av bevisene i systemet.
create trigger company_member_change_append_only
    before update or delete on company_member_change
    for each row execute function forbid_ledger_mutation();
create trigger company_member_change_no_truncate
    before truncate on company_member_change
    for each statement execute function forbid_ledger_mutation();

-- Applikasjonsrollen: opprette og lese invitasjoner, og bare merke dem
-- som brukt eller tilbakekalt. En invitasjon kan ikke omskrives til en
-- annen rolle eller en annen adresse etter at den er sendt.
grant select, insert on company_invitation to regnmed_app;
grant update (accepted_at, accepted_by, revoked_at, revoked_by)
    on company_invitation to regnmed_app;

grant select, insert on company_member_change to regnmed_app;
