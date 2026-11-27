-- Notice of new customer terms (docs/kundevilkar.md §14.1, #185).
--
-- The terms promise that a change is announced in regnmed and by e-mail
-- before it takes effect. A row here is that e-mail: which company was
-- told about which version, at which address, and when. One per company
-- and version, so the daily run tells each company once.
--
-- The id doubles as the mail's Nats-Msg-Id, like dispatch (0020).
-- Insert-only: a notice that was sent is evidence, and evidence is not
-- edited.
create table terms_notice (
    id            uuid primary key,
    company_id    uuid not null references company (id),
    terms_sha256  text not null check (terms_sha256 ~ '^[0-9a-f]{64}$'),
    recipient     text not null check (recipient <> ''),
    sent_at       timestamptz not null default now(),
    unique (company_id, terms_sha256)
);

create trigger terms_notice_append_only
    before update or delete on terms_notice
    for each row execute function forbid_ledger_mutation();
create trigger terms_notice_no_truncate
    before truncate on terms_notice
    for each statement execute function forbid_ledger_mutation();

grant select, insert on terms_notice to regnmed_app;
