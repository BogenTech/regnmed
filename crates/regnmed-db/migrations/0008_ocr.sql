-- OCR giro innbetalinger: imported payment batches (oppdrag) and their
-- KID-tagged payments. Files are dokumentasjon — insert-only for the app
-- role, idempotent on (transmission, assignment) so a re-uploaded file
-- is rejected instead of duplicated. Application of payments against
-- invoices arrives with reskontro; until then payments are matched
-- against bank statement lines by the reconciliation view.

create table ocr_batch (
    id                  uuid primary key,
    company_id          uuid not null references company (id),
    -- Ledger account the payments settle into (typically the bank account).
    account_id          uuid not null references account (id),
    transmission_number text not null,
    assignment_number   text not null,
    agreement_id        text not null,
    -- The 11-digit oppdragskonto from the file.
    bank_account        text not null,
    imported_by         text not null check (imported_by <> ''),
    created_at          timestamptz not null default now(),
    unique (company_id, transmission_number, assignment_number)
);

create table ocr_payment (
    id                 uuid primary key,
    batch_id           uuid not null references ocr_batch (id),
    transaction_number text not null,
    payment_date       date not null,
    amount_ore         bigint not null,
    kid                text not null,
    kid_valid          boolean not null,
    transaction_type   text not null,
    bank_reference     text,
    debit_account      text
);

create index ocr_payment_batch_idx on ocr_payment (batch_id);

grant select, insert on ocr_batch, ocr_payment to regnmed_app;
