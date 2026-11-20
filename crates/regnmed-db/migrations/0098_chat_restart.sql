-- «Ny samtale» with an agent (docs/meldinger.md): a mark from which the
-- agent starts fresh. Every new order to an agent carries the recent
-- conversation, and the agent trusts what it said before -- so one wrong
-- answer kept coming back. The messages before the mark stay (they are
-- the trail); they are only no longer sent with the next order.
create table chat_restart (
    id              uuid primary key,
    conversation_id uuid not null references chat_conversation (id),
    person_id       uuid not null references person (id),
    created_at      timestamptz not null default now()
);

create index chat_restart_conversation_idx on chat_restart (conversation_id, created_at);

create trigger chat_restart_append_only
    before update or delete on chat_restart
    for each row execute function forbid_ledger_mutation();
create trigger chat_restart_no_truncate
    before truncate on chat_restart
    for each statement execute function forbid_ledger_mutation();

grant select, insert on chat_restart to regnmed_app;
