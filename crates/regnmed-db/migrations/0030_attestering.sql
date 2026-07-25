-- Attestering: godkjenningsflyt før bokføring og betaling
-- (docs/attestering.md, #47). Større SMB-er skiller hvem som MOTTAR en
-- kostnad, hvem som GODKJENNER den og hvem som BOKFØRER/BETALER —
-- intern kontroll som førsteklasses flyt.
--
-- To tabeller, samme ærlighetsdisiplin som resten:
--
-- attestation_policy — valgfri policy per selskap, append-only historikk
--   (nyeste rad gjelder, mva_terminordning-mønsteret). Når policyen er
--   aktiv: innboksbilag over beløpsgrensen krever godkjent attestering
--   av en ANNEN person før bokføring; betalingslister krever alltid en
--   annen godkjenner enn oppretteren (fire øyne på penger ut); utlegg
--   kan ikke godkjennes av den som sendte dem inn. En utpekt attestant
--   (attestant_person_id) begrenser hvem som kan attestere; NULL betyr
--   alle med bokføringstilgang.
--
-- attestation — insert-only beslutningsspor på målet (hvem, når,
--   godkjent/avvist + notat). Nyeste beslutning gjelder; historikken er
--   nøyaktig det et ettersyn spør etter. Håndhevingen skjer server-side
--   i bokfør/godkjenn-transaksjonene, aldri bare i UI.

create table attestation_policy (
    id                  uuid primary key,
    company_id          uuid not null references company (id),
    aktiv               boolean not null,
    -- Innboksbilag med debetsum >= grensen krever attestering.
    -- NULL = alle bilag krever attestering når policyen er aktiv.
    belopsgrense_ore    bigint check (belopsgrense_ore is null or belopsgrense_ore >= 0),
    -- Utpekt attestant; NULL = alle med bokføringstilgang kan attestere.
    attestant_person_id uuid references person (id),
    created_by          text not null check (created_by <> ''),
    created_at          timestamptz not null default now()
);

create index attestation_policy_company_idx
    on attestation_policy (company_id, created_at desc);

create trigger attestation_policy_append_only
    before update or delete on attestation_policy
    for each row execute function forbid_ledger_mutation();
create trigger attestation_policy_no_truncate
    before truncate on attestation_policy
    for each statement execute function forbid_ledger_mutation();

grant select, insert on attestation_policy to regnmed_app;

create table attestation (
    id                uuid primary key,
    company_id        uuid not null references company (id),
    target_kind       text not null check (target_kind in ('inbox_document')),
    target_id         uuid not null,
    decision          text not null check (decision in ('godkjent', 'avvist')),
    note              text,
    decided_by_person uuid not null references person (id),
    decided_by        text not null check (decided_by <> ''),
    created_at        timestamptz not null default now(),
    -- En avvisning uten begrunnelse er ikke en beslutning.
    check (decision <> 'avvist' or (note is not null and note <> ''))
);

create index attestation_target_idx
    on attestation (company_id, target_kind, target_id, created_at desc);

create trigger attestation_append_only
    before update or delete on attestation
    for each row execute function forbid_ledger_mutation();
create trigger attestation_no_truncate
    before truncate on attestation
    for each statement execute function forbid_ledger_mutation();

grant select, insert on attestation to regnmed_app;

-- Fire øyne på betalingslister trenger oppretterens IDENTITET, ikke
-- bare visningsnavnet: nye kjøringer bærer person-id-en. Gamle rader
-- (NULL) kan ikke godkjennes under aktiv policy — lag listen på nytt.
alter table payment_run add column created_by_person uuid references person (id);

-- Oppretteren er en del av kjøringens uforanderlige innhold, som
-- created_by: utvid vakten fra 0029 til å dekke den nye kolonnen.
create or replace function payment_run_guard() returns trigger
language plpgsql as $$
begin
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.debitor_konto is distinct from old.debitor_konto
       or new.execution_date is distinct from old.execution_date
       or new.created_by is distinct from old.created_by
       or new.created_by_person is distinct from old.created_by_person
       or new.created_at is distinct from old.created_at then
        raise exception 'payment run content is immutable';
    end if;
    if old.status = 'utkast' and new.status in ('godkjent', 'annullert') then
        return new;
    end if;
    if old.status = 'godkjent' and new.status = 'utbetalt'
       and new.approved_by = old.approved_by and new.approved_at = old.approved_at
       and new.file = old.file and new.file_sha256 = old.file_sha256 then
        return new;
    end if;
    raise exception 'payment run % kan ikke gå fra % til %', old.id, old.status, new.status;
end;
$$;
