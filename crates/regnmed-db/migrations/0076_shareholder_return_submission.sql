-- The aksjonærregisteroppgave submission record (docs/aksjonaer.md,
-- «Innsending», #182).
--
-- One filing is three kinds of call -- hovedskjema, one underskjema per
-- shareholder, bekreft -- each with its own idempotencyKey. A retry of a
-- filing that never got its receipt must resend EVERY call with the SAME
-- key and the SAME body, so Skatteetaten answers each with its first
-- answer instead of starting a second oppgave. That only works if what is
-- sent is stored before it is sent: these rows are written first, the
-- HTTP calls second. The a-melding record (0074) is the same rule.
--
-- Insert-only, like the ledger. Status is computed from the events.
create table shareholder_return_submission (
    id              uuid primary key,
    company_id      uuid not null references company (id),
    income_year     integer not null check (income_year between 2000 and 2100),
    -- The hovedskjema XML, byte for byte: what is resent, and the evidence
    -- of what was reported.
    main_form       text not null check (main_form <> ''),
    main_key        uuid not null unique,
    confirm_key     uuid not null unique,
    created_by      text not null check (created_by <> ''),
    created_at      timestamptz not null default now()
);

create index shareholder_return_submission_year_idx
    on shareholder_return_submission (company_id, income_year, created_at desc);

create table shareholder_return_submission_form (
    submission_id   uuid not null references shareholder_return_submission (id),
    -- Sending order; bekreft states the count.
    position        integer not null check (position >= 0),
    shareholder_id  uuid not null references shareholder (id),
    xml             text not null check (xml <> ''),
    idempotency_key uuid not null unique,
    primary key (submission_id, position)
);

create table shareholder_return_submission_event (
    id               uuid primary key,
    submission_id    uuid not null references shareholder_return_submission (id),
    -- receipt: Skatteetaten confirmed the oppgave (the Leveransebekreftelse).
    -- failed:  no receipt; the SAME submission is retried with the same keys.
    kind             text not null check (kind in ('receipt', 'failed')),
    dialog_id        text,
    forsendelse_id   text,
    -- oppgavegiversLeveranseReferanse.
    reference        text,
    detail           jsonb,
    created_by       text not null check (created_by <> ''),
    created_at       timestamptz not null default now(),
    check (kind <> 'receipt' or (dialog_id is not null and forsendelse_id is not null)),
    check (kind <> 'failed' or detail is not null)
);

-- One receipt per submission: a second would be a second delivery the
-- record cannot explain.
create unique index shareholder_return_submission_one_receipt
    on shareholder_return_submission_event (submission_id) where kind = 'receipt';

create trigger shareholder_return_submission_append_only
    before update or delete on shareholder_return_submission
    for each row execute function forbid_ledger_mutation();
create trigger shareholder_return_submission_no_truncate
    before truncate on shareholder_return_submission
    for each statement execute function forbid_ledger_mutation();
create trigger shareholder_return_submission_form_append_only
    before update or delete on shareholder_return_submission_form
    for each row execute function forbid_ledger_mutation();
create trigger shareholder_return_submission_form_no_truncate
    before truncate on shareholder_return_submission_form
    for each statement execute function forbid_ledger_mutation();
create trigger shareholder_return_submission_event_append_only
    before update or delete on shareholder_return_submission_event
    for each row execute function forbid_ledger_mutation();
create trigger shareholder_return_submission_event_no_truncate
    before truncate on shareholder_return_submission_event
    for each statement execute function forbid_ledger_mutation();

grant select, insert on shareholder_return_submission to regnmed_app;
grant select, insert on shareholder_return_submission_form to regnmed_app;
grant select, insert on shareholder_return_submission_event to regnmed_app;
