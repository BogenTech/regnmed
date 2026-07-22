-- Reskontro: kunde- og leverandørspesifikasjon (bokføringsforskriften
-- §3-1). Parties are master data; the party binding on ledger entries is
-- part of the hash chain (format v2) so receivables cannot be silently
-- reassigned between customers — see docs/ledger.md.

create table party (
    id         uuid primary key,
    company_id uuid not null references company (id),
    -- Numeric business identifier (kundenummer/leverandørnummer); part
    -- of the v2 hash, so it is immutable once postings reference it.
    party_no   text not null check (party_no ~ '^[0-9]+$'),
    kind       text not null check (kind in ('kunde', 'leverandor')),
    name       text not null check (name <> ''),
    orgnr      text check (orgnr ~ '^[0-9]{9}$'),
    created_at timestamptz not null default now(),
    unique (company_id, party_no)
);

-- Accounts flagged as reskontro accounts (1500 kundefordringer, 2400
-- leverandørgjeld) require a matching party on every posting.
alter table account add column reskontro_kind text
    check (reskontro_kind in ('kunde', 'leverandor'));

-- Hash chain format version per voucher: 1 = original (frozen), 2 = adds
-- the party number per entry. Existing history stays 1 and verifies
-- unchanged forever; new postings write 2.
alter table voucher add column hash_version smallint not null default 1;

alter table entry add column party_id uuid references party (id);

create index entry_party_idx on entry (party_id);

-- Open-item matching (åpne poster): pairs an invoice-side entry with a
-- settlement-side entry for an amount (partial settlements allowed). An
-- entry is open while its matched amount is below its own amount —
-- computed, never stored.
create table reskontro_match (
    id         uuid primary key,
    entry_a    uuid not null references entry (id),
    entry_b    uuid not null references entry (id),
    amount_ore bigint not null check (amount_ore > 0),
    matched_by text not null check (matched_by <> ''),
    created_at timestamptz not null default now(),
    check (entry_a <> entry_b)
);

create index reskontro_match_a_idx on reskontro_match (entry_a);
create index reskontro_match_b_idx on reskontro_match (entry_b);

grant select, insert on party to regnmed_app;
grant update (name, orgnr) on party to regnmed_app;
grant update (reskontro_kind) on account to regnmed_app;
grant select, insert, delete on reskontro_match to regnmed_app;
