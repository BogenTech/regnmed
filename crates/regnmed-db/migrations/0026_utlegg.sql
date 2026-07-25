-- Utlegg og kjøregodtgjørelse (docs/utlegg.md, #42).
--
-- The innboks discipline applied to reimbursement claims:
-- - The claim's CONTENT (receipt, amounts, satser) is immutable from
--   submission — SHA-256 stored at upload, column grants + trigger.
-- - Decisions are one-way: innsendt → godkjent (with the kostnad
--   voucher) or → avvist (note required); godkjent → utbetalt (with
--   the payment voucher). Nothing is re-decided, nothing is deleted.
-- - Kjøregodtgjørelse stores the satser it was computed with — a rate
--   change never touches a submitted claim (the row is evidence).

create table expense (
    id                   uuid primary key,
    company_id           uuid not null references company (id),
    person_id            uuid not null references person (id),
    kind                 text not null check (kind in ('utlegg', 'kjoring')),
    dato                 date not null,
    beskrivelse          text not null check (beskrivelse <> ''),
    belop_ore            bigint not null check (belop_ore > 0),
    -- Utlegg: the receipt (oppbevaringsplikt — copied onto the voucher
    -- as an attachment at approval).
    receipt_filename     text,
    receipt_content_type text,
    receipt_content      bytea,
    receipt_sha256       bytea check (receipt_sha256 is null or octet_length(receipt_sha256) = 32),
    -- Kjøring: kilometre and the satser valid on `dato`, stored at
    -- submission. The trekkpliktige part awaits lønn/a-melding (#46)
    -- and is surfaced as a warning, never hidden.
    km                   bigint check (km is null or km > 0),
    sats_ore_per_km      bigint,
    trekkfri_ore         bigint,
    trekkpliktig_ore     bigint,
    status               text not null default 'innsendt'
        check (status in ('innsendt', 'godkjent', 'avvist', 'utbetalt')),
    decided_by           text,
    decided_at           timestamptz,
    avvist_note          text,
    -- Set at approval: the kostnad voucher and the mellomregningskonto
    -- it credited (the payment later debits the same konto).
    voucher_id           uuid references voucher (id),
    motkonto             text,
    utbetalt_voucher_id  uuid references voucher (id),
    utbetalt_at          timestamptz,
    created_by           text not null check (created_by <> ''),
    created_at           timestamptz not null default now(),
    check (kind <> 'utlegg' or (receipt_content is not null and receipt_sha256 is not null
                                and receipt_filename is not null and receipt_content_type is not null)),
    check (kind <> 'kjoring' or (km is not null and sats_ore_per_km is not null
                                 and trekkfri_ore is not null and trekkpliktig_ore is not null)),
    check (kind = 'utlegg' or receipt_content is null),
    check (status <> 'godkjent' or (voucher_id is not null and motkonto is not null)),
    check (status <> 'utbetalt' or (voucher_id is not null and utbetalt_voucher_id is not null)),
    check (status <> 'avvist' or (avvist_note is not null and avvist_note <> ''))
);

create index expense_company_idx on expense (company_id, status, created_at desc);

create function expense_guard() returns trigger
language plpgsql as $$
begin
    -- Content is immutable from submission; only the workflow fields
    -- may ever change, and only along the one-way transitions.
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.person_id is distinct from old.person_id
       or new.kind is distinct from old.kind
       or new.dato is distinct from old.dato
       or new.beskrivelse is distinct from old.beskrivelse
       or new.belop_ore is distinct from old.belop_ore
       or new.receipt_filename is distinct from old.receipt_filename
       or new.receipt_content_type is distinct from old.receipt_content_type
       or new.receipt_content is distinct from old.receipt_content
       or new.receipt_sha256 is distinct from old.receipt_sha256
       or new.km is distinct from old.km
       or new.sats_ore_per_km is distinct from old.sats_ore_per_km
       or new.trekkfri_ore is distinct from old.trekkfri_ore
       or new.trekkpliktig_ore is distinct from old.trekkpliktig_ore
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'expense content is immutable from submission';
    end if;
    if old.status = 'innsendt' and new.status in ('godkjent', 'avvist') then
        return new;
    end if;
    if old.status = 'godkjent' and new.status = 'utbetalt'
       and new.decided_by = old.decided_by and new.decided_at = old.decided_at
       and new.voucher_id = old.voucher_id and new.motkonto = old.motkonto then
        return new;
    end if;
    raise exception 'expense % kan ikke gå fra % til %', old.id, old.status, new.status;
end;
$$;

create trigger expense_update_guard
    before update on expense
    for each row execute function expense_guard();
create trigger expense_no_delete
    before delete on expense
    for each row execute function forbid_ledger_mutation();
create trigger expense_no_truncate
    before truncate on expense
    for each statement execute function forbid_ledger_mutation();

grant select, insert on expense to regnmed_app;
grant update (status, decided_by, decided_at, avvist_note, voucher_id, motkonto,
              utbetalt_voucher_id, utbetalt_at)
    on expense to regnmed_app;
