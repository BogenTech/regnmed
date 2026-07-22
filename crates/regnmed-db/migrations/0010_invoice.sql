-- Utgående faktura (salgsdokument, bokføringsforskriften §5-1).
--
-- Invoices are immutable once issued (insert-only for the app role; a
-- mistake is corrected with a kreditnota, never an edit) and numbered
-- gap-free per company via a counter bumped in the same transaction as
-- the ledger posting — invoice number and voucher number roll back
-- together. The KID is derived from the invoice number (MOD10) and
-- unique per company, which lets OCR payments identify their invoice.

create table invoice_counter (
    company_id  uuid primary key references company (id),
    last_number bigint not null check (last_number > 0)
);

create table invoice (
    id                   uuid primary key,
    company_id           uuid not null references company (id),
    party_id             uuid not null references party (id),
    invoice_no           bigint not null,
    invoice_date         date not null,
    due_date             date not null,
    kid                  text not null,
    -- Set on kreditnotaer: the invoice this one credits.
    credits_invoice_id   uuid references invoice (id),
    -- The ledger posting this invoice generated.
    voucher_id           uuid not null references voucher (id),
    -- The receivable entry (party line) — reskontro remaining lives there.
    receivable_entry_id  uuid not null references entry (id),
    created_by           text not null check (created_by <> ''),
    created_at           timestamptz not null default now(),
    unique (company_id, invoice_no),
    unique (company_id, kid)
);

create table invoice_line (
    id             uuid primary key,
    invoice_id     uuid not null references invoice (id),
    line_no        integer not null check (line_no > 0),
    description    text not null check (description <> ''),
    account_number text not null,
    -- Quantity in thousandths (2.5 stk = 2500); negative on kreditnotaer.
    quantity_milli bigint not null check (quantity_milli <> 0),
    unit_price_ore bigint not null,
    net_ore        bigint not null,
    vat_code       text references vat_code (code),
    vat_ore        bigint not null,
    unique (invoice_id, line_no)
);

alter table ocr_payment add column invoice_id uuid references invoice (id);

grant select, insert on invoice, invoice_line to regnmed_app;
grant select, insert, update on invoice_counter to regnmed_app;
