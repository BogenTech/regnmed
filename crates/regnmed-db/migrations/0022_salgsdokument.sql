-- Tilbud → ordre → faktura (docs/faktura.md, #31).
--
-- The commercial chain BEFORE the invoice lives OUTSIDE the ledger:
-- nothing posts, tilbud are freely editable until akseptert/avslått,
-- and an ordre is a frozen confirmation. Each kind has its own
-- gap-free number series per company (same counter pattern as
-- invoices) — a rejected tilbud is history, not a hole. Converting an
-- ordre runs the normal atomic invoice path and links the chain
-- (tilbud id → ordre id → invoice id) for traceability.

create table salgsdokument (
    id           uuid primary key,
    company_id   uuid not null references company (id),
    kind         text not null check (kind in ('tilbud', 'ordre')),
    doc_no       bigint not null,
    party_id     uuid not null references party (id),
    doc_date     date not null,
    status       text not null check (
        (kind = 'tilbud' and status in ('utkast', 'sendt', 'akseptert', 'avslatt'))
        or (kind = 'ordre' and status in ('bekreftet', 'fakturert'))
    ),
    -- On an ordre: the accepted tilbud it came from (at most one ordre
    -- per tilbud — enforced below).
    tilbud_id    uuid references salgsdokument (id),
    -- On an ordre once fakturert: the invoice it became.
    invoice_id   uuid references invoice (id),
    created_by   text not null check (created_by <> ''),
    created_at   timestamptz not null default now(),
    updated_at   timestamptz not null default now(),
    unique (company_id, kind, doc_no)
);

create unique index salgsdokument_one_order_per_tilbud
    on salgsdokument (tilbud_id) where tilbud_id is not null;

create table salgsdokument_line (
    id             uuid primary key,
    dokument_id    uuid not null references salgsdokument (id),
    line_no        integer not null check (line_no > 0),
    description    text not null check (description <> ''),
    account_number text not null,
    quantity_milli bigint not null check (quantity_milli <> 0),
    unit_price_ore bigint not null,
    vat_code       text references vat_code (code),
    avdeling       text,
    prosjekt       text,
    unique (dokument_id, line_no)
);

create table salgsdokument_counter (
    company_id  uuid not null references company (id),
    kind        text not null check (kind in ('tilbud', 'ordre')),
    last_number bigint not null check (last_number > 0),
    primary key (company_id, kind)
);

grant select, insert, update on salgsdokument to regnmed_app;
grant select, insert, update, delete on salgsdokument_line to regnmed_app;
grant select, insert, update on salgsdokument_counter to regnmed_app;
