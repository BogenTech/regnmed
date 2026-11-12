-- Review of #198/#199 (docs/lonn.md «Skattekort fra Skatteetaten»).
--
-- A skattekort order was recorded only AFTER Skatteetaten accepted it
-- (0078: tax_card_order.reference is not null and comes from the reply),
-- so an order Skatteetaten refused -- or lost on the way -- left no trace,
-- although every employee's fødselsnummer had been sent. The terms' §7.1
-- register must show every call. So, like the a-melding (0074): the
-- request is written FIRST, the call made second, and the outcome added
-- after -- `tax_card_order` (same id) when Skatteetaten accepted,
-- `tax_card_request_failure` when it did not or rejected the answer.
create table tax_card_request (
    -- Also the idempotensid sent, and the id of its tax_card_order.
    id              uuid primary key,
    company_id      uuid not null references company (id),
    income_year     integer not null check (income_year between 2000 and 2100),
    employee_count  integer not null check (employee_count between 1 and 1000),
    requested_by    text not null check (requested_by <> ''),
    created_at      timestamptz not null default now()
);

create index tax_card_request_company_idx on tax_card_request (company_id, created_at desc);

create table tax_card_request_failure (
    id          uuid primary key,
    request_id  uuid not null references tax_card_request (id),
    -- Skatteetaten's answer or the transport error, verbatim. Read by the
    -- payroll rights only -- the register says THAT it failed.
    detail      text not null check (detail <> ''),
    created_by  text not null check (created_by <> ''),
    created_at  timestamptz not null default now()
);

create index tax_card_request_failure_idx on tax_card_request_failure (request_id);

-- Orders from before this migration got their request after the fact.
insert into tax_card_request (id, company_id, income_year, employee_count, requested_by, created_at)
select id, company_id, income_year, employee_count, requested_by, created_at
  from tax_card_order;

-- From now on no order exists without the request written before it.
alter table tax_card_order
    add constraint tax_card_order_request_fk foreign key (id) references tax_card_request (id);

create trigger tax_card_request_append_only
    before update or delete on tax_card_request
    for each row execute function forbid_ledger_mutation();
create trigger tax_card_request_no_truncate
    before truncate on tax_card_request
    for each statement execute function forbid_ledger_mutation();
create trigger tax_card_request_failure_append_only
    before update or delete on tax_card_request_failure
    for each row execute function forbid_ledger_mutation();
create trigger tax_card_request_failure_no_truncate
    before truncate on tax_card_request_failure
    for each statement execute function forbid_ledger_mutation();

grant select, insert on tax_card_request to regnmed_app;
grant select, insert on tax_card_request_failure to regnmed_app;

-- The register reads every event of a submission; the 0074/0076 indexes
-- are partial (one kind each) and could not serve it.
create index amelding_submission_event_submission_idx
    on amelding_submission_event (submission_id);
create index shareholder_return_submission_event_submission_idx
    on shareholder_return_submission_event (submission_id);
