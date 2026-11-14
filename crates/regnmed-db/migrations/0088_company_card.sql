-- Company cards (docs/utlegg.md «Firmakort»).
--
-- A purchase on the company's card is the company's money, not the
-- employee's: its receipt is documentation, not a claim to be paid. It
-- goes through the same draft -> submitted -> approved flow (the innboks
-- discipline holds the receipt), but approval credits the CARD's ledger
-- account instead of the debt to the employee, and there is nothing left
-- to pay out.
--
-- Each card has its own ledger account (a debit card on the bank account
-- it draws on, a credit card on an account of its own), so the card's
-- statement is imported and reconciled like any bank statement. A
-- statement line with no receipt behind it is a receipt the cardholder
-- still owes; a claim filed from that line (bank_transaction_id) is
-- matched to it at approval, in the same transaction.

create table company_card (
    id               uuid primary key,
    company_id       uuid not null references company (id),
    name             text not null check (name <> ''),
    last4            text not null check (last4 ~ '^[0-9]{4}$'),
    -- Who carries it; null = a shared card only approvers file for.
    holder_person_id uuid references person (id),
    -- Permanent: approved purchases are posted against it.
    account_id       uuid not null references account (id),
    active           boolean not null default true,
    created_by       text not null check (created_by <> ''),
    created_at       timestamptz not null default now()
);

-- One card per account, so a statement line names its card.
create unique index company_card_account_once on company_card (account_id) where active;
create index company_card_company_idx on company_card (company_id);

create trigger company_card_no_delete
    before delete on company_card
    for each row execute function forbid_ledger_mutation();

grant select, insert on company_card to regnmed_app;
grant update (name, holder_person_id, active) on company_card to regnmed_app;

alter table expense
    add column company_card_id uuid references company_card (id),
    add column bank_transaction_id uuid references bank_transaction (id),
    add constraint expense_card_kind_check
        check (kind = 'utlegg' or (company_card_id is null and bank_transaction_id is null)),
    add constraint expense_card_line_check
        check (bank_transaction_id is null or company_card_id is not null);

-- A statement line is documented by one receipt; a rejected or discarded
-- claim frees it for the next try.
create unique index expense_bank_transaction_once on expense (bank_transaction_id)
    where bank_transaction_id is not null and status not in ('avvist', 'forkastet');

create or replace function expense_guard() returns trigger
language plpgsql as $$
begin
    if is_content_offload(to_jsonb(old), to_jsonb(new), 'receipt_content', 'receipt_sha256') then
        return new;
    end if;
    -- Identity never moves, draft or not.
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.person_id is distinct from old.person_id
       or new.registered_by_person is distinct from old.registered_by_person
       or new.kind is distinct from old.kind
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'expense identity is immutable';
    end if;
    if old.status = 'utkast' then
        -- A draft has no decision trail, and gets none on the way in.
        if new.decided_by is not null or new.decided_at is not null
           or new.avvist_note is not null or new.voucher_id is not null
           or new.motkonto is not null or new.utbetalt_voucher_id is not null
           or new.utbetalt_at is not null then
            raise exception 'expense % is a draft and has no decision', old.id;
        end if;
        if new.status in ('utkast', 'innsendt', 'forkastet') then
            return new;
        end if;
        raise exception 'expense % kan ikke gå fra utkast til %', old.id, new.status;
    end if;
    -- From submission on: the content is evidence.
    if new.dato is distinct from old.dato
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
       or new.department is distinct from old.department
       or new.project is distinct from old.project
       or new.billable is distinct from old.billable
       or new.route_from is distinct from old.route_from
       or new.route_to is distinct from old.route_to
       or new.round_trip is distinct from old.round_trip
       or new.vehicle is distinct from old.vehicle
       or new.passengers is distinct from old.passengers
       or new.passenger_rate_ore_per_km is distinct from old.passenger_rate_ore_per_km
       or new.trip_expense_id is distinct from old.trip_expense_id
       or new.departure_at is distinct from old.departure_at
       or new.return_at is distinct from old.return_at
       or new.destination is distinct from old.destination
       or new.overnight is distinct from old.overnight
       or new.over_15_km is distinct from old.over_15_km
       or new.travel_days is distinct from old.travel_days
       or new.rate_scheme is distinct from old.rate_scheme
       or new.travel_lines is distinct from old.travel_lines
       or new.company_card_id is distinct from old.company_card_id
       or new.bank_transaction_id is distinct from old.bank_transaction_id then
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

grant update (company_card_id, bank_transaction_id) on expense to regnmed_app;
