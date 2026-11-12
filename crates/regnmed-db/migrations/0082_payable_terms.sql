-- Forfallsdato og KID på leverandørposter (docs/betaling.md).
--
-- A supplier invoice is booked through the inbox as an ordinary voucher;
-- its due date and KID were read (EHF, bilagstolkning) and then dropped,
-- so the payment list paid without KID and nothing could say what falls
-- due when. They are facts read off the supplier's document, not ledger
-- content: the hash chain and the entry rows stay untouched, and these
-- rows sit beside the entry.
--
-- Insert-only. A wrong due date or KID is corrected by a NEW row; the
-- newest row for an entry applies, and the old ones stay as the record of
-- who said what, when.
create table payable_terms (
    id          uuid primary key,
    company_id  uuid not null references company (id),
    entry_id    uuid not null references entry (id),
    due_date    date,
    kid         text check (kid ~ '^[0-9]{2,25}-?$'),
    recorded_by text not null check (recorded_by <> ''),
    recorded_at timestamptz not null default now(),
    check (due_date is not null or kid is not null)
);
create index payable_terms_entry_idx on payable_terms (entry_id, recorded_at desc, id desc);

grant select, insert on payable_terms to regnmed_app;

-- Insert-only in the database itself, whatever the role (the grants above
-- cover the application; this covers everyone else).
create function payable_terms_insert_only() returns trigger
language plpgsql as $$
begin
    raise exception
        'payable_terms is insert-only: % is not allowed — record a new row instead',
        tg_op;
end;
$$;
create trigger payable_terms_no_change
    before update or delete on payable_terms
    for each row execute function payable_terms_insert_only();
create trigger payable_terms_no_truncate
    before truncate on payable_terms
    for each statement execute function payable_terms_insert_only();
