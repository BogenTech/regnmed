-- Per-agent extra instructions (docs/agent.md §8.6).
--
-- The role prompt says what a bokfører does; this says what THIS agent
-- does on top of it -- the company's own account numbers, who to ask,
-- which suppliers are routine. Editable, like the name and the face:
-- an instruction that cannot be corrected is an instruction nobody
-- dares to write.
--
-- It is text handed to a model, never authorization. Whatever it says,
-- the role on the grant still decides what the agent may do, enforced
-- where every other caller is enforced -- so a prompt cannot widen
-- access, only narrow what the agent attempts.
alter table integration add column systemprompt text check (systemprompt <> '');

grant update (systemprompt) on integration to regnmed_app;
