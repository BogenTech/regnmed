-- Altinn Events subscriptions regnmed has created (docs/lonn.md,
-- «Tilbakemelding som hendelse»): per company, so the a-melding feedback
-- arrives at our webhook instead of being asked for. Insert-only; the
-- unique key makes "is this company subscribed?" one lookup and stops a
-- second subscription for the same company, filter and endpoint.
create table altinn_event_subscription (
    id              uuid primary key,
    company_id      uuid not null references company (id),
    -- Altinn's own subscription id (GET/DELETE /subscriptions/{id}).
    altinn_id       bigint not null,
    resource_filter text not null check (resource_filter <> ''),
    type_filter     text not null check (type_filter <> ''),
    end_point       text not null check (end_point like 'https://%'),
    created_by      text not null check (created_by <> ''),
    created_at      timestamptz not null default now(),
    unique (company_id, resource_filter, type_filter, end_point)
);

create trigger altinn_event_subscription_append_only
    before update or delete on altinn_event_subscription
    for each row execute function forbid_ledger_mutation();
create trigger altinn_event_subscription_no_truncate
    before truncate on altinn_event_subscription
    for each statement execute function forbid_ledger_mutation();

grant select, insert on altinn_event_subscription to regnmed_app;
