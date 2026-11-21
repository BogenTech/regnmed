-- A company's approval of regnmed as a system in Altinn (docs/gov.md).
-- The customer approves in Altinn, so this row only mirrors where that
-- stands. It is operational state; the authority keeps the real grant.
create table altinn_system_user (
    company_id        uuid primary key references company (id),
    -- Null when Altinn reported an approval that predates this table.
    altinn_request_id text unique check (altinn_request_id <> ''),
    status            text not null check (status <> ''),
    confirm_url       text,
    requested_by      text not null check (requested_by <> ''),
    created_at        timestamptz not null default now(),
    updated_at        timestamptz not null default now()
);

grant select, insert, update on altinn_system_user to regnmed_app;
