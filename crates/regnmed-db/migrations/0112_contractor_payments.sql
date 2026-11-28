-- Payments to self-employed people without a fixed place of business
-- (third-party report, formerly RF-1321; docs/selvstendig.md, #363).
--
-- 1. `contractor_assessment`: the customer's judgement, per supplier,
--    that it is (or is not) self-employed without a fixed place of
--    business. Skatteetaten leaves that judgement to the payer, so regnmed
--    never infers it: only a row here puts a supplier in the report. The
--    newest row per supplier applies; earlier rows are the history of who
--    decided what, and when. A fødselsnummer (or D-nummer) is stored only
--    for a supplier without an orgnr: the rettledning says the orgnr is
--    used when there is one.
-- 2. `contractor_report_submission` and its events: the 0076 rule -- what
--    is sent is stored before it is sent, with the idempotency key it is
--    always sent with, so a retry never files twice.
--
-- Insert-only, like the ledger.

create table contractor_assessment (
    id               uuid primary key,
    company_id       uuid not null,
    party_id         uuid not null,
    self_employed    boolean not null,
    nin              text check (nin ~ '^[0-9]{11}$'),
    decided_by       text not null check (decided_by <> ''),
    created_at       timestamptz not null default now(),
    -- Same company as the supplier (0047's composite key).
    foreign key (company_id, party_id) references party (company_id, id),
    check (self_employed or nin is null)
);
create index contractor_assessment_party_idx
    on contractor_assessment (company_id, party_id, created_at desc);

create table contractor_report_submission (
    id               uuid primary key,
    company_id       uuid not null references company (id),
    income_year      integer not null check (income_year between 2000 and 2100),
    -- The JSON as sent, byte for byte: what is resent, and the evidence
    -- of what was reported. Its oppgavegiversLeveranseReferanse is this id.
    document         text not null check (document <> ''),
    idempotency_key  uuid not null unique,
    created_by       text not null check (created_by <> ''),
    created_at       timestamptz not null default now()
);
create index contractor_report_submission_year_idx
    on contractor_report_submission (company_id, income_year, created_at desc);

create table contractor_report_submission_event (
    id               uuid primary key,
    submission_id    uuid not null references contractor_report_submission (id),
    -- receipt: the Leveransebekreftelse. failed: no receipt, retried as
    -- stored. outcome: Skatteetaten's answer from Dialogporten, verbatim.
    kind             text not null check (kind in ('receipt', 'failed', 'outcome')),
    dialog_id        text,
    forsendelse_id   text,
    detail           jsonb,
    created_by       text not null check (created_by <> ''),
    created_at       timestamptz not null default now(),
    check (kind <> 'receipt' or (dialog_id is not null and forsendelse_id is not null)),
    check (kind = 'receipt' or detail is not null)
);
create unique index contractor_report_submission_one_receipt
    on contractor_report_submission_event (submission_id) where kind = 'receipt';
create unique index contractor_report_submission_one_outcome
    on contractor_report_submission_event (submission_id) where kind = 'outcome';

create trigger contractor_assessment_append_only
    before update or delete on contractor_assessment
    for each row execute function forbid_ledger_mutation();
create trigger contractor_assessment_no_truncate
    before truncate on contractor_assessment
    for each statement execute function forbid_ledger_mutation();
create trigger contractor_report_submission_append_only
    before update or delete on contractor_report_submission
    for each row execute function forbid_ledger_mutation();
create trigger contractor_report_submission_no_truncate
    before truncate on contractor_report_submission
    for each statement execute function forbid_ledger_mutation();
create trigger contractor_report_submission_event_append_only
    before update or delete on contractor_report_submission_event
    for each row execute function forbid_ledger_mutation();
create trigger contractor_report_submission_event_no_truncate
    before truncate on contractor_report_submission_event
    for each statement execute function forbid_ledger_mutation();

grant select, insert on contractor_assessment to regnmed_app;
grant select, insert on contractor_report_submission to regnmed_app;
grant select, insert on contractor_report_submission_event to regnmed_app;
