-- Ledger immutability, grants layer: the application connects as (a member
-- of) regnmed_app, which cannot UPDATE or DELETE ledger rows even if the
-- append-only triggers were somehow dropped. Deployments create a LOGIN
-- user and `grant regnmed_app to <user>;`.
--
-- Migrations must run as a privileged role (the owner), never as the app.

do $$
begin
    if not exists (select from pg_roles where rolname = 'regnmed_app') then
        create role regnmed_app nologin;
    end if;
end;
$$;

grant usage on schema public to regnmed_app;

-- Ledger history: append and read only.
grant select, insert on voucher, entry to regnmed_app;

-- Master data: insert and read; edits are column-restricted below.
grant select, insert on company, journal, account to regnmed_app;
grant select on vat_code to regnmed_app;

-- Master data stays editable, but identity columns are fixed once created:
-- account numbers, journal codes, orgnr and company links cannot change.
grant update (name, vat_code, active) on account to regnmed_app;
grant update (name) on journal to regnmed_app;
grant update (name) on company to regnmed_app;

-- Mutable pointers (not history): the chain tip and voucher counters.
grant select, insert, update on chain_head, voucher_counter to regnmed_app;
