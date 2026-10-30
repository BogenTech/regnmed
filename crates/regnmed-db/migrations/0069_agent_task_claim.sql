-- Exactly one worker on a task at a time (docs/agent.md §8.7).
--
-- The CronJob could not need this: `concurrencyPolicy: Forbid` meant one
-- run at a time for the whole platform, which is also why agents could
-- not scale past a single task. With a work queue and N workers that
-- ceiling is gone, and so is the accident that was holding the line.
--
-- A JetStream work queue already delivers a message to one consumer, so
-- why a second guard? Because `ack_wait` has to be long -- a model turn
-- takes minutes -- and a redelivery after it expires would put a second
-- worker on a task the first is still running. That task posts vouchers.
-- At-least-once delivery plus an unguarded ledger write is the one
-- combination this system exists to prevent.
--
-- So the queue is a doorbell and this table is the lock. Claiming is one
-- statement (insert .. on conflict do update .. where stale), which is
-- atomic: either you hold the row or somebody else does, and no sequence
-- of readers can both conclude they won.
--
-- Rows are DELETED on completion rather than kept: this is a lease, not
-- evidence. What the agent did is in agent_oppgave_hendelse, which is
-- insert-only and stays that way.
create table agent_task_claim (
    oppgave_id uuid primary key references agent_oppgave (id),
    -- Which process holds it; for an operator reading a stuck queue.
    worker     text        not null check (worker <> ''),
    claimed_at timestamptz not null default now()
);

grant select, insert, update, delete on agent_task_claim to regnmed_app;
