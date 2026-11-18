-- Who gave an agent its task, as a PERSON, not only as a display name
-- (docs/agent.md §8.5). `opprettet_av` is the name the trail shows; it
-- cannot reach anyone. When an agent asks a question, the person who gave
-- the task is told by e-mail, and that needs an address to send to.
--
-- Nullable: tasks given before this, and tasks that start in a chat
-- conversation (where the question already lands in front of the person),
-- have none. The table stays insert-only (0064): the column is set once,
-- at creation.
alter table agent_oppgave add column created_by_person uuid references person (id);
