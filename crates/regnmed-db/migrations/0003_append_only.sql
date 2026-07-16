-- Ledger immutability, database layer: the database itself refuses
-- UPDATE, DELETE and TRUNCATE on ledger history — regardless of role.
-- Corrections are posted as reversing vouchers (voucher.reverses_voucher_id).

create function forbid_ledger_mutation() returns trigger
language plpgsql as $$
begin
    raise exception
        'the ledger is append-only: % on % is not allowed — post a reversing voucher instead',
        tg_op, tg_table_name;
end;
$$;

create trigger voucher_append_only
    before update or delete on voucher
    for each row execute function forbid_ledger_mutation();

create trigger voucher_no_truncate
    before truncate on voucher
    for each statement execute function forbid_ledger_mutation();

create trigger entry_append_only
    before update or delete on entry
    for each row execute function forbid_ledger_mutation();

create trigger entry_no_truncate
    before truncate on entry
    for each statement execute function forbid_ledger_mutation();
