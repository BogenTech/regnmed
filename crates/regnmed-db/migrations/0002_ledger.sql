-- The ledger: an append-only, hash-chained journal of vouchers (bilag).
--
-- Immutability is layered:
--   1. Domain:   corrections are reversing vouchers, never edits.
--   2. Database: UPDATE/DELETE/TRUNCATE on voucher/entry is rejected by
--                trigger (migration 0003), and the application role is only
--                granted INSERT/SELECT (migration 0004).
--   3. Crypto:   every voucher stores hash = SHA-256(prev_hash || content),
--                computed in regnmed-core. `regnmed verify-ledger` re-walks
--                the chain from genesis; anchoring the head hash outside
--                the database makes even DBA-level tampering detectable.

-- The tip of each company's hash chain. This row is a mutable *pointer*
-- (it moves on every posting), not history. Verification never trusts it:
-- the chain is recomputed from the vouchers and only compared against it.
-- Locking this row FOR UPDATE serializes postings per company, which the
-- chain requires anyway.
create table chain_head (
    company_id uuid primary key references company (id),
    last_seq   bigint not null check (last_seq >= 0),
    last_hash  bytea not null check (octet_length(last_hash) = 32)
);

-- Gap-free voucher numbering per journal and fiscal year. A plain sequence
-- can leave gaps on rollback; this counter is bumped inside the posting
-- transaction, so a rolled-back posting rolls its number back too.
create table voucher_counter (
    journal_id  uuid not null references journal (id),
    fiscal_year integer not null,
    last_number bigint not null check (last_number > 0),
    primary key (journal_id, fiscal_year)
);

create table voucher (
    id                  uuid primary key,
    company_id          uuid not null references company (id),
    journal_id          uuid not null references journal (id),
    fiscal_year         integer not null,
    voucher_number      bigint not null,
    voucher_date        date not null,
    description         text not null check (description <> ''),
    -- Set when this voucher reverses an earlier one (correction).
    reverses_voucher_id uuid references voucher (id),
    created_by          text not null check (created_by <> ''),
    -- Set by the application (truncated to microseconds) because it is part
    -- of the hashed content and must round-trip through storage exactly.
    created_at          timestamptz not null,
    chain_seq           bigint not null check (chain_seq > 0),
    prev_hash           bytea not null check (octet_length(prev_hash) = 32),
    hash                bytea not null check (octet_length(hash) = 32),
    unique (journal_id, fiscal_year, voucher_number),
    unique (company_id, chain_seq)
);

create index voucher_company_date_idx on voucher (company_id, voucher_date);

-- Amounts are in øre (1/100 NOK): positive = debit, negative = credit.
-- Account balances are always SUM(amount_ore) over entries — never stored
-- mutable state. Materialized per-period balances may come later as a pure
-- cache; the journal stays the single source of truth.
create table entry (
    id          uuid primary key,
    voucher_id  uuid not null references voucher (id),
    line_no     integer not null check (line_no > 0),
    account_id  uuid not null references account (id),
    amount_ore  bigint not null check (amount_ore <> 0),
    vat_code    text references vat_code (code) check (vat_code <> ''),
    description text check (description <> ''),
    unique (voucher_id, line_no)
);

create index entry_account_idx on entry (account_id);

-- Double-entry invariant, re-checked by the database at commit (deferred,
-- so lines can be inserted one by one): every voucher has at least two
-- entry lines and they sum to exactly zero.
create function assert_voucher_balanced() returns trigger
language plpgsql as $$
declare
    v_id    uuid;
    v_sum   bigint;
    v_count integer;
begin
    if tg_table_name = 'voucher' then
        v_id := new.id;
    else
        v_id := new.voucher_id;
    end if;

    select coalesce(sum(amount_ore), 0), count(*)
      into v_sum, v_count
      from entry
     where voucher_id = v_id;

    if v_count < 2 then
        raise exception 'voucher % has % entry lines; at least two are required', v_id, v_count;
    end if;
    if v_sum <> 0 then
        raise exception 'voucher % does not balance: entries sum to % øre', v_id, v_sum;
    end if;
    return null;
end;
$$;

create constraint trigger voucher_balanced
    after insert on voucher
    deferrable initially deferred
    for each row execute function assert_voucher_balanced();

create constraint trigger entry_balanced
    after insert on entry
    deferrable initially deferred
    for each row execute function assert_voucher_balanced();
