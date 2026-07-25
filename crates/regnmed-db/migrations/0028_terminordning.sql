-- Mva-terminordning per selskap (docs/mva.md, #51). To-måneder er
-- standard og trenger ingen rad; en INNVILGET ordning (årstermin,
-- primærnæring) registreres med virkning fra en dato — datert som alt
-- annet regelverksaktig, append-only så historikken alltid viser
-- hvilken ordning som gjaldt når. Skifte tilbake er en ny rad.
-- Systemet avgjør aldri berettigelse — mennesket registrerer det
-- Skatteetaten har innvilget (note-feltet bærer referansen).

create table mva_terminordning (
    company_id uuid not null references company (id),
    valid_from date not null,
    ordning    text not null check (ordning in ('to-maneder', 'arlig', 'primaernaering')),
    note       text,
    created_by text not null check (created_by <> ''),
    created_at timestamptz not null default now(),
    primary key (company_id, valid_from)
);

create trigger mva_terminordning_append_only
    before update or delete on mva_terminordning
    for each row execute function forbid_ledger_mutation();
create trigger mva_terminordning_no_truncate
    before truncate on mva_terminordning
    for each statement execute function forbid_ledger_mutation();

grant select, insert on mva_terminordning to regnmed_app;
