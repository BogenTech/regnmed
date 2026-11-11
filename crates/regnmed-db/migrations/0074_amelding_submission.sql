-- The a-melding submission record (docs/lonn.md, «Innsendingssporet»).
--
-- Two meldinger for the same month ADD UP at Skatteetaten (seen in the
-- test environment 2026-11-11), so a retry must resend the SAME body with
-- the SAME idempotency key, and a correction must be a replacement
-- (erstatterMeldingsId). That only works if what was sent is stored
-- before it is sent: this table is written first, the HTTP call second.
--
-- Insert-only, like the ledger. What happened to a submission is the
-- event log below; its status is computed from it, never stored.
create table amelding_submission (
    id                  uuid primary key,
    company_id          uuid not null references company (id),
    run_id              uuid not null references payroll_run (id),
    -- The meldingsId sent. New per submission (a replacement needs a new
    -- one), so NOT the run id.
    melding_id          text not null unique,
    idempotency_key     uuid not null unique,
    -- Kalendermåned: the month of the payment date.
    period              text not null check (period ~ '^[0-9]{4}-(0[1-9]|1[0-2])$'),
    -- erstatterMeldingsId: the earlier delivered submission this replaces.
    replaces_melding_id text references amelding_submission (melding_id),
    -- The exact JSON sent, byte for byte: a retry resends THIS, and it is
    -- the evidence of what was reported.
    body                text not null check (body <> ''),
    created_by          text not null check (created_by <> ''),
    created_at          timestamptz not null default now()
);

create index amelding_submission_run_idx
    on amelding_submission (company_id, run_id, created_at desc);

create table amelding_submission_event (
    id             uuid primary key,
    submission_id  uuid not null references amelding_submission (id),
    -- receipt: Skatteetaten took it (dialogId/forsendelseId).
    -- failed:  the call did not get a receipt; the SAME submission is retried.
    -- feedback: Skatteetaten's tilbakemelding, verbatim.
    kind           text not null check (kind in ('receipt', 'failed', 'feedback')),
    dialog_id      text,
    forsendelse_id text,
    detail         jsonb,
    created_by     text not null check (created_by <> ''),
    created_at     timestamptz not null default now(),
    check (kind <> 'receipt' or (dialog_id is not null and forsendelse_id is not null)),
    check (kind = 'receipt' or detail is not null)
);

-- One receipt and one feedback per submission: the second of either
-- would be a second delivery the record cannot explain.
create unique index amelding_submission_one_receipt
    on amelding_submission_event (submission_id) where kind = 'receipt';
create unique index amelding_submission_one_feedback
    on amelding_submission_event (submission_id) where kind = 'feedback';

create trigger amelding_submission_append_only
    before update or delete on amelding_submission
    for each row execute function forbid_ledger_mutation();
create trigger amelding_submission_no_truncate
    before truncate on amelding_submission
    for each statement execute function forbid_ledger_mutation();
create trigger amelding_submission_event_append_only
    before update or delete on amelding_submission_event
    for each row execute function forbid_ledger_mutation();
create trigger amelding_submission_event_no_truncate
    before truncate on amelding_submission_event
    for each statement execute function forbid_ledger_mutation();

grant select, insert on amelding_submission to regnmed_app;
grant select, insert on amelding_submission_event to regnmed_app;
