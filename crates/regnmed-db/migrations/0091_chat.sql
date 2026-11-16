-- Meldinger: one-to-one conversations (docs/meldinger.md).
--
-- People talk to people: employees to colleagues, clients to their
-- byrå's contact person and back, and anyone to the company's agents
-- (an agent IS a user, #101). WHO may talk to whom is not stored here --
-- it is computed from company_member, firm_member, engagement and
-- integration_grant every time (`chat::contacts`), so ending an
-- engagement or a membership ends the conversation at once, the same
-- way it ends access to the books.
--
-- Same doctrine as the rest of the trail: a message is never edited or
-- deleted, a correction is a new message. The database is the record;
-- NATS only rings the doorbell (regnmed-mail `chatbus`).

create table chat_conversation (
    id          uuid primary key,
    -- Set when one side is an agent. An agent acts in one company -- its
    -- grant's -- and that is where the tasks it is given must go.
    company_id  uuid references company (id),
    -- Two people (plus the company, for an agent) have exactly ONE
    -- conversation: the sorted person ids, so a:b and b:a collide.
    direct_key  text not null unique,
    created_at  timestamptz not null default now()
);

create trigger chat_conversation_append_only
    before update or delete on chat_conversation
    for each row execute function forbid_ledger_mutation();

grant select, insert on chat_conversation to regnmed_app;

create table chat_participant (
    conversation_id uuid not null references chat_conversation (id),
    person_id       uuid not null references person (id),
    -- How far this person has read. The one thing here that moves.
    read_until      timestamptz,
    primary key (conversation_id, person_id)
);

create index chat_participant_person_idx on chat_participant (person_id);

grant select, insert on chat_participant to regnmed_app;
grant update (read_until) on chat_participant to regnmed_app;

create table chat_message (
    id               uuid primary key,
    conversation_id  uuid not null references chat_conversation (id),
    author_person_id uuid not null references person (id),
    -- The name as it was when it was said, like `created_by` everywhere.
    author_name      text not null check (author_name <> ''),
    body             text not null check (body <> '' and length(body) <= 8000),
    -- A message to an agent IS an order: the agent_oppgave it created or
    -- continued, so the thread can show what the agent is doing and the
    -- task trail can show who asked.
    task_id          uuid references agent_oppgave (id),
    created_at       timestamptz not null default now()
);

create index chat_message_conversation_idx on chat_message (conversation_id, created_at desc);
create index chat_message_task_idx on chat_message (task_id) where task_id is not null;

create trigger chat_message_append_only
    before update or delete on chat_message
    for each row execute function forbid_ledger_mutation();
create trigger chat_message_no_truncate
    before truncate on chat_message
    for each statement execute function forbid_ledger_mutation();

grant select, insert on chat_message to regnmed_app;

-- An attachment IS an inbox document of one company: a photo sent with
-- "bokfør denne" belongs where it can be booked, hashed and kept (#96),
-- not in a second file store beside the innboks.
create table chat_message_document (
    message_id  uuid not null references chat_message (id),
    document_id uuid not null references inbox_document (id),
    primary key (message_id, document_id)
);

create trigger chat_message_document_append_only
    before update or delete on chat_message_document
    for each row execute function forbid_ledger_mutation();

grant select, insert on chat_message_document to regnmed_app;

-- Which of the byrå's members a client can write to, per engagement.
-- Chosen by the byrå's admin; an empty or missing row means "the byrå's
-- admins", so a new engagement is reachable from day one.
create table engagement_contact (
    engagement_id uuid primary key references engagement (id),
    person_ids    uuid[] not null,
    set_by        text not null check (set_by <> ''),
    set_at        timestamptz not null default now()
);

grant select, insert, update on engagement_contact to regnmed_app;
