-- Customer terms acceptance (docs/kundevilkar.md §1.4, #185).
--
-- Digdir's and Skatteetaten's terms for end-user system vendors allow no
-- filing on a customer's behalf without an agreement with that customer.
-- A row here is that agreement: which company accepted, who accepted for
-- it, and EXACTLY which text -- the SHA-256 of the terms as served. A new
-- text has a new hash and needs a new acceptance.
--
-- Insert-only, like the ledger: an acceptance is evidence, and evidence
-- is not edited.
create table terms_acceptance (
    id            uuid primary key,
    company_id    uuid not null references company (id),
    person_id     uuid not null references person (id),
    terms_sha256  text not null check (terms_sha256 ~ '^[0-9a-f]{64}$'),
    -- The accepting person's name at the time; the person row may change.
    accepted_by   text not null check (accepted_by <> ''),
    accepted_at   timestamptz not null default now(),
    -- The first acceptance of a text is the one that counts.
    unique (company_id, terms_sha256)
);

create trigger terms_acceptance_append_only
    before update or delete on terms_acceptance
    for each row execute function forbid_ledger_mutation();
create trigger terms_acceptance_no_truncate
    before truncate on terms_acceptance
    for each statement execute function forbid_ledger_mutation();

grant select, insert on terms_acceptance to regnmed_app;
