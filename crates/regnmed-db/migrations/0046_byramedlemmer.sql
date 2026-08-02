-- Byråmedlemmer (#78, docs/marketplace.md).
--
-- Fram til nå ble `firm_member` skrevet av nøyaktig én kodevei:
-- registreringen gjorde grunnleggeren til admin — og der stoppet det.
-- Byrået med tre ansatte kunne ikke slippe dem inn. Tilgangsmodellen
-- var klar hele tiden (hvert aktivt byråmedlem når byråets klienter
-- gjennom oppdragene); det som manglet var medlemsadministrasjonen.
--
-- Mønsteret er 0037 for selskaper, med de samme begrunnelsene:
-- invitasjonen er stilet til en E-POSTADRESSE (personen finnes ikke før
-- første innlogging, og et «finnes denne adressen»-oppslag ville røpet
-- hvem som er bruker), og endringssporet er innsettings-bart.

create table firm_invitation (
    id          uuid primary key,
    firm_id     uuid not null references firm (id),
    -- Normalisert (trimmet og små bokstaver) av applikasjonen.
    epost       text not null check (epost <> ''),
    -- Byråroller fra 0005: admin styrer byrået, ansatt jobber i det.
    role        text not null check (role in ('admin', 'ansatt')),
    invited_by  uuid not null references person (id),
    created_at  timestamptz not null default now(),
    accepted_at timestamptz,
    accepted_by uuid references person (id),
    revoked_at  timestamptz,
    revoked_by  uuid references person (id)
);

-- Én åpen invitasjon per byrå og adresse; brukte og tilbakekalte blir
-- liggende som historikk.
create unique index firm_invitation_open_uq
    on firm_invitation (firm_id, epost)
    where accepted_at is null and revoked_at is null;

create index firm_invitation_epost_idx
    on firm_invitation (epost)
    where accepted_at is null and revoked_at is null;

-- «Hvem slapp denne personen inn i byrået, og når» — byråets klienter
-- er selskaper med revisorer, så spørsmålet stilles her også.
create table firm_member_change (
    id         uuid primary key,
    firm_id    uuid not null references firm (id),
    person_id  uuid not null references person (id),
    endring    text not null check (
                   endring in ('lagt_til', 'rolle_endret', 'deaktivert', 'reaktivert')),
    fra_rolle  text,
    til_rolle  text,
    -- NULL når personen selv løste inn en invitasjon — avsenderen står
    -- da i firm_invitation.invited_by.
    utfort_av  uuid references person (id),
    kilde      text not null check (kilde in ('admin', 'invitasjon', 'registrering')),
    created_at timestamptz not null default now()
);

create index firm_member_change_idx
    on firm_member_change (firm_id, created_at desc);

create trigger firm_member_change_append_only
    before update or delete on firm_member_change
    for each row execute function forbid_ledger_mutation();
create trigger firm_member_change_no_truncate
    before truncate on firm_member_change
    for each statement execute function forbid_ledger_mutation();

-- Invitasjonsmailen rir samme skinne og samme logg som all annen
-- utgående e-post (0044). En byråinvitasjon har ikke noe selskap, så
-- selskapskolonnen åpnes for NULL — men bare når raden peker på en
-- byråinvitasjon; enhver annen utsendelse krever fortsatt sitt selskap.
alter table utsendelse
    add column firm_invitation_id uuid references firm_invitation (id);
alter table utsendelse alter column company_id drop not null;

alter table utsendelse drop constraint utsendelse_check;
alter table utsendelse add constraint utsendelse_check
    check (
        (company_id is not null
            and (invoice_id is not null
                or reminder_id is not null
                or invitation_id is not null))
        or (company_id is null and firm_invitation_id is not null)
    );

create index utsendelse_firm_invitation on utsendelse (firm_invitation_id);

-- Applikasjonsrollen: som 0037 — en sendt invitasjon kan bare merkes
-- brukt eller tilbakekalt, aldri omskrives; sporet kan bare vokse.
grant select, insert on firm_invitation to regnmed_app;
grant update (accepted_at, accepted_by, revoked_at, revoked_by)
    on firm_invitation to regnmed_app;

grant select, insert on firm_member_change to regnmed_app;
