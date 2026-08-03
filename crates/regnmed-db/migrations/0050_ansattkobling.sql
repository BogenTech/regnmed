-- Ansatt ↔ portalbruker: koblingen får en vei inn og et spor (docs/lonn.md).
--
-- 0036 ga `employee.person_id` og sa hvordan den IKKE skal settes (aldri
-- gjettet fra navn). Denne migrasjonen gir de to veiene den SKAL settes:
--
--   1. Invitasjonen bærer den ansatte: admin oppgir e-postadressen når
--      den ansatte registreres, og innløsningen i /me setter koblingen i
--      SAMME transaksjon som medlemskapet. Da er det den ansatte selv
--      som kobler seg, ved å logge inn med sin egen adresse — admin
--      velger aldri en person fra en liste.
--   2. Manuell kobling til et EKSISTERENDE medlem, gjort av admin.
--
-- Begge veier etterlater en rad i `employee_link_change`: en feil
-- kobling betyr at noen kan lese en annens lønnsslipp, så hvem som
-- koblet hvem må kunne besvares i ettertid. Omkobling er et to-stegs
-- bevisst valg — koble fra først, så koble på nytt — aldri en glipp i
-- en nedtrekksliste.

alter table company_invitation
    add column employee_id uuid references employee (id);

-- Insert-only trail of link and unlink events. `utfort_av` is NULL when
-- the person redeemed an invitation themselves (the inviter stands on
-- the invitation row, as with memberships).
create table employee_link_change (
    id          uuid primary key default gen_random_uuid(),
    company_id  uuid not null references company (id),
    employee_id uuid not null references employee (id),
    person_id   uuid not null references person (id),
    endring     text not null check (endring in ('koblet', 'frakoblet')),
    kilde       text not null check (kilde in ('invitasjon', 'admin')),
    utfort_av   uuid references person (id),
    created_at  timestamptz not null default now()
);

create index employee_link_change_idx
    on employee_link_change (employee_id, created_at desc);

create trigger employee_link_change_append_only
    before update or delete on employee_link_change
    for each row execute function forbid_ledger_mutation();
create trigger employee_link_change_no_truncate
    before truncate on employee_link_change
    for each statement execute function forbid_ledger_mutation();

grant select, insert on employee_link_change to regnmed_app;
