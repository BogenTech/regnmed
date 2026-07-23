-- External anchoring of chain heads (M6 trust work).
--
-- An anchor snapshot freezes every company's chain head (last_seq +
-- last_hash) as leaves of a Merkle tree; the single root is what leaves
-- the database (transparency endpoint, RFC 3161 witness tokens). The
-- tables are evidence and therefore append-only like the ledger itself:
-- an adversary who can rewrite chains must also erase the anchors, and
-- the roots already published externally still convict them.

create table anchor_snapshot (
    id         uuid primary key,
    created_at timestamptz not null default now(),
    root_hash  bytea not null check (octet_length(root_hash) = 32),
    leaf_count int not null check (leaf_count > 0)
);

create index anchor_snapshot_created_idx on anchor_snapshot (created_at desc);

create table anchor_leaf (
    snapshot_id uuid not null references anchor_snapshot (id),
    company_id  uuid not null references company (id),
    last_seq    bigint not null check (last_seq > 0),
    last_hash   bytea not null check (octet_length(last_hash) = 32),
    primary key (snapshot_id, company_id)
);

create index anchor_leaf_company_idx on anchor_leaf (company_id);

-- One row per successful external publication of a snapshot's root:
-- method 'rfc3161' stores the DER TimeStampResp token, verifiable
-- offline with openssl ts (docs/anchoring.md).
create table anchor_witness (
    id           uuid primary key,
    snapshot_id  uuid not null references anchor_snapshot (id),
    method       text not null check (method <> ''),
    reference    text not null,
    proof        bytea not null,
    witnessed_at timestamptz not null default now()
);

create index anchor_witness_snapshot_idx on anchor_witness (snapshot_id);

create trigger anchor_snapshot_append_only
    before update or delete on anchor_snapshot
    for each row execute function forbid_ledger_mutation();
create trigger anchor_snapshot_no_truncate
    before truncate on anchor_snapshot
    for each statement execute function forbid_ledger_mutation();

create trigger anchor_leaf_append_only
    before update or delete on anchor_leaf
    for each row execute function forbid_ledger_mutation();
create trigger anchor_leaf_no_truncate
    before truncate on anchor_leaf
    for each statement execute function forbid_ledger_mutation();

create trigger anchor_witness_append_only
    before update or delete on anchor_witness
    for each row execute function forbid_ledger_mutation();
create trigger anchor_witness_no_truncate
    before truncate on anchor_witness
    for each statement execute function forbid_ledger_mutation();

grant select, insert on anchor_snapshot, anchor_leaf, anchor_witness to regnmed_app;
