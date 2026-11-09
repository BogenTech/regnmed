-- The read side of the ledger (docs/hovedbok.md, "Lesemodellen"): debit
-- and credit per account per calendar month, so a report sums a few
-- hundred rows instead of every entry the company ever posted. Measured
-- 2026-11-09 at 40 000 entries: trial balance 79 ms, and the account
-- specification's opening balance (recomputed per row) 20 s for one month.
--
-- This is a CACHE, never a source of truth: the journal stays the only
-- record, and `regnmed verify-ledger` checks every row against
-- SUM(entry.amount_ore). It is kept exact by construction rather than
-- eventually: the trigger below runs inside the posting transaction, so a
-- voucher and its totals commit or roll back together, and no reader ever
-- sees one without the other. Postings are already serialized per company
-- by the chain_head lock, so the upsert adds no new contention.
--
-- Only the trigger writes here. The function is SECURITY DEFINER and the
-- app role is granted SELECT alone, so application code cannot write a
-- total that disagrees with the entries -- the same layering as the
-- ledger itself. Entries are append-only (0003), so a total only grows.
create table account_month_total (
    company_id  uuid   not null references company (id),
    account_id  uuid   not null references account (id),
    month       date   not null check (month = date_trunc('month', month)::date),
    debit_ore   bigint not null check (debit_ore >= 0),
    credit_ore  bigint not null check (credit_ore <= 0),
    entry_count bigint not null check (entry_count > 0),
    primary key (account_id, month)
);

create index account_month_total_company on account_month_total (company_id, month);

create function account_month_total_add() returns trigger
language plpgsql security definer set search_path = public as $$
begin
    insert into account_month_total (company_id, account_id, month, debit_ore, credit_ore, entry_count)
    select v.company_id, new.account_id, date_trunc('month', v.voucher_date)::date,
           greatest(new.amount_ore, 0), least(new.amount_ore, 0), 1
    from voucher v
    where v.id = new.voucher_id
    on conflict (account_id, month) do update
       set debit_ore   = account_month_total.debit_ore + excluded.debit_ore,
           credit_ore  = account_month_total.credit_ore + excluded.credit_ore,
           entry_count = account_month_total.entry_count + 1;
    return null;
end;
$$;

-- Created BEFORE the backfill: creating the trigger locks entry against
-- inserts until this migration commits, so no posting can land between
-- the backfill's snapshot and the trigger taking over.
create trigger entry_account_month_total
    after insert on entry
    for each row execute function account_month_total_add();

insert into account_month_total (company_id, account_id, month, debit_ore, credit_ore, entry_count)
select v.company_id, e.account_id, date_trunc('month', v.voucher_date)::date,
       coalesce(sum(e.amount_ore) filter (where e.amount_ore > 0), 0),
       coalesce(sum(e.amount_ore) filter (where e.amount_ore < 0), 0),
       count(*)
from entry e
join voucher v on v.id = e.voucher_id
group by v.company_id, e.account_id, date_trunc('month', v.voucher_date);

grant select on account_month_total to regnmed_app;
