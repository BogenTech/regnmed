-- SAF-T import log (docs/migration.md, multi-year import).
--
-- With one file per fiscal year the import door stays open across
-- several uploads, and the opening-balance reconciliation is the guard
-- against files that do not continue the history. It has one blind
-- spot: a file whose period nets to zero on every account reconciles
-- cleanly a second time and would double-post its transactions. The
-- log closes the byte-identical case — the same content can never be
-- imported twice into the same company — and doubles as the audit
-- trail of WHICH files a migrated ledger was built from.

create table saft_import_log (
    id             uuid primary key,
    company_id     uuid not null references company (id),
    -- SHA-256 of the uploaded SAF-T XML (before any kontoplan mapping
    -- is applied — the mapping changes the interpretation, not the
    -- source document).
    content_sha256 bytea not null check (octet_length(content_sha256) = 32),
    accounts       integer not null,
    vouchers       integer not null,
    opening_posted boolean not null,
    created_by     text not null check (created_by <> ''),
    created_at     timestamptz not null default now(),
    unique (company_id, content_sha256)
);

create trigger saft_import_log_append_only
    before update or delete on saft_import_log
    for each row execute function forbid_ledger_mutation();
create trigger saft_import_log_no_truncate
    before truncate on saft_import_log
    for each statement execute function forbid_ledger_mutation();

grant select, insert on saft_import_log to regnmed_app;
