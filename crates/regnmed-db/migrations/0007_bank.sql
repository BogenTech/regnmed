-- Bank reconciliation: imported statements, their transactions, and
-- matches against ledger entries.
--
-- Statements are dokumentasjon (bokføringsloven §13): insert-only for the
-- app role, and duplicates are rejected on the bank's own statement id.
-- Matches are workflow state: a match may be created and removed, and
-- "unmatched" is always computed as the absence of a match row — never
-- stored mutable state.

create table bank_statement (
    id           uuid primary key,
    company_id   uuid not null references company (id),
    -- The ledger bank account (e.g. 1920) this statement reconciles.
    account_id   uuid not null references account (id),
    -- The bank's own statement id; makes re-import idempotent.
    statement_ref text not null check (statement_ref <> ''),
    iban         text,
    from_date    date,
    to_date      date,
    opening_ore  bigint,
    closing_ore  bigint,
    imported_by  text not null check (imported_by <> ''),
    created_at   timestamptz not null default now(),
    unique (company_id, statement_ref)
);

create table bank_transaction (
    id           uuid primary key,
    statement_id uuid not null references bank_statement (id),
    booking_date date not null,
    -- Ledger sign for the bank account: money in = positive (debit).
    amount_ore   bigint not null check (amount_ore <> 0),
    description  text not null default '',
    reference    text
);

create index bank_transaction_statement_idx on bank_transaction (statement_id);

create table bank_match (
    bank_transaction_id uuid primary key references bank_transaction (id),
    entry_id            uuid not null unique references entry (id),
    method              text not null check (method in ('auto', 'manual')),
    matched_by          text not null check (matched_by <> ''),
    created_at          timestamptz not null default now()
);

grant select, insert on bank_statement, bank_transaction to regnmed_app;
grant select, insert, delete on bank_match to regnmed_app;
