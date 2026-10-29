-- The agent's own OIDC client secret, sealed (docs/agent.md §8.3).
--
-- A hosted agent authenticates to our API exactly like any other
-- integration: client_credentials from regnid, and `integration_grant`
-- decides the rest. That is the whole point -- the worker runs the loop
-- and holds the model key, but it does NOT get a second way into the
-- ledger. There is one authentication path and robots use the same one
-- people do.
--
-- The client_id is already here: it is the person's `oidc_sub`. This is
-- only the secret, sealed by regnmed-core::hemmelig like the provider
-- key, so the database holds ciphertext and nothing in it can open it.
--
-- Updatable rather than insert-only, unlike agent_llm. A trail of
-- superseded provider CHOICES is evidence worth keeping; a trail of
-- superseded SECRETS is a liability. Rotating replaces it.
alter table integration add column klient_hemmelighet bytea;

grant update (klient_hemmelighet) on integration to regnmed_app;
