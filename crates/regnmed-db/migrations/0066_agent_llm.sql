-- The customer's own LLM provider, per agent (docs/agent.md §8).
--
-- Scoped to (integration, company) rather than to the integration
-- alone. An integration is one identity that may hold grants in several
-- companies, and `navn` is shared across them -- but a provider key is
-- somebody's BILL and somebody's data-processor choice, so company A
-- must not be able to configure the model company B's work runs on, or
-- hand B its own invoice.
--
-- Insert-only, newest row wins: the dated-row pattern this repo uses
-- for attestation_policy, abonnement_pris and sats. It costs no extra
-- table and answers the question that matters afterwards -- which
-- provider was our ledger sent to, and when, and who decided it.
--
-- `nokkel` is sealed by regnmed-core::hemmelig before it gets here. The
-- sealing key is configuration on the process, never a row: this table
-- holds ciphertext and nothing in the database can open it.
create table agent_llm (
    id             uuid primary key,
    integration_id uuid not null references integration (id),
    company_id     uuid not null references company (id),
    -- 'openai', 'openrouter' or 'annet'. Kept as text, not an enum: the
    -- list of providers is a product decision that will change more
    -- often than this schema should.
    leverandor     text not null check (leverandor <> ''),
    base_url       text not null check (base_url <> ''),
    modell         text not null check (modell <> ''),
    nokkel         bytea,
    -- Turning the agent's model off is a new row, not a delete, so the
    -- trail keeps saying what was true before it.
    aktiv          boolean not null default true,
    satt_av        text not null check (satt_av <> ''),
    created_at     timestamptz not null default now(),
    check (not aktiv or nokkel is not null)
);

create index agent_llm_nyeste_idx
    on agent_llm (integration_id, company_id, created_at desc);

create trigger agent_llm_append_only
    before update or delete on agent_llm
    for each row execute function forbid_ledger_mutation();
create trigger agent_llm_no_truncate
    before truncate on agent_llm
    for each statement execute function forbid_ledger_mutation();

grant select, insert on agent_llm to regnmed_app;
