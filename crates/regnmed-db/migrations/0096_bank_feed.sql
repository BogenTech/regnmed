-- Direct bank connection (docs/bank.md tier 2, #70): statements fetched
-- from the bank through an aggregator instead of uploaded as files. It is
-- TRANSPORT around the existing engine: what arrives becomes an ordinary
-- `bank_statement` and is matched like any other.

-- Which add-ons (tilleggstjenester, migration 0093) a company has, as dated
-- coverage rows like `abonnement`: granting is an insert, ending sets
-- `valid_to` (EXCLUSIVE), nothing is deleted. Buying one in the portal and
-- invoicing it are not built yet; a row is written by the platform console.
create table company_addon (
    id         uuid primary key default gen_random_uuid(),
    company_id uuid not null references company (id),
    addon      text not null check (addon <> ''),
    valid_from date not null default current_date,
    valid_to   date check (valid_to is null or valid_to > valid_from),
    -- Why the row exists (agreement, pilot). Never empty.
    note       text not null check (note <> ''),
    created_by text not null check (created_by <> ''),
    created_at timestamptz not null default now()
);

create unique index company_addon_open_idx on company_addon (company_id, addon)
    where valid_to is null;

grant select, insert on company_addon to regnmed_app;
grant update (valid_to) on company_addon to regnmed_app;

create trigger company_addon_no_delete
    before delete on company_addon
    for each row execute function forbid_ledger_mutation();
create trigger company_addon_no_truncate
    before truncate on company_addon
    for each statement execute function forbid_ledger_mutation();

-- One consent to read one bank account, feeding one ledger bank account.
-- The row is created when the user is sent to the bank (its id is the
-- `state` that comes back), completed when they return, and ended by the
-- user, by a renewal, or when the bank says the consent is gone. What is
-- stored is the provider's session reference: never bank credentials.
create table bank_connection (
    id                  uuid primary key,
    company_id          uuid not null references company (id),
    account_id          uuid not null references account (id),
    provider            text not null check (provider in ('enablebanking')),
    bank_name           text not null check (bank_name <> ''),
    bank_country        text not null default 'NO',
    -- The bank account the user said this is (11 digits). The account the
    -- bank returns must be this one, so a consent for another account
    -- can never feed this ledger account.
    bban                text not null check (bban ~ '^[0-9]{11}$'),
    session_id          text,
    external_account_id text,
    valid_until         timestamptz,
    last_synced_at      timestamptz,
    last_error          text,
    ended_at            timestamptz,
    end_reason          text,
    created_by          text not null check (created_by <> ''),
    created_at          timestamptz not null default now(),
    check ((session_id is null) = (external_account_id is null)),
    check ((ended_at is null) = (end_reason is null))
);

-- At most one live, completed connection per ledger account.
create unique index bank_connection_live_idx on bank_connection (account_id)
    where ended_at is null and session_id is not null;
create index bank_connection_company_idx on bank_connection (company_id, created_at desc);

grant select, insert on bank_connection to regnmed_app;
grant update (session_id, external_account_id, valid_until, last_synced_at,
              last_error, ended_at, end_reason) on bank_connection to regnmed_app;

create trigger bank_connection_no_delete
    before delete on bank_connection
    for each row execute function forbid_ledger_mutation();

-- Every transaction the feed has imported, per LEDGER ACCOUNT (not per
-- connection: a renewed consent is a new connection for the same account).
-- Fetch windows overlap on purpose, and this is what keeps an overlap from
-- becoming a second copy. Written in the same transaction as the statement.
create table bank_feed_seen (
    account_id   uuid not null references account (id),
    key          text not null check (key <> ''),
    statement_id uuid not null references bank_statement (id),
    primary key (account_id, key)
);

grant select, insert on bank_feed_seen to regnmed_app;

create trigger bank_feed_seen_append_only
    before update or delete on bank_feed_seen
    for each row execute function forbid_ledger_mutation();
