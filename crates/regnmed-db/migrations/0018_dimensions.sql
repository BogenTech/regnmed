-- Dimensjoner: avdeling og prosjekt på posteringene (docs/dimensjoner.md).
--
-- The registry is master data with a restricted lifecycle: insert +
-- rename + open/close only — code, kind and company can never change
-- once postings may reference them (the code is inside the v3 voucher
-- hash; a renamed CODE would break chain verification, a renamed NAME
-- does not). Enforced for the app role via column grants and for
-- everyone via trigger.
--
-- Entry references are nullable FKs; the dimension CODE (not the id) is
-- covered by hash format v3, so verification re-reads codes via join
-- exactly as it does party numbers.

create table dimension (
    id         uuid primary key,
    company_id uuid not null references company (id),
    kind       text not null check (kind in ('avdeling', 'prosjekt')),
    code       text not null check (code <> ''),
    name       text not null check (name <> ''),
    active     boolean not null default true,
    created_at timestamptz not null default now(),
    unique (company_id, kind, code)
);

create function forbid_dimension_identity_change() returns trigger
language plpgsql as $$
begin
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.kind is distinct from old.kind
       or new.code is distinct from old.code
       or new.created_at is distinct from old.created_at then
        raise exception 'dimension identity is immutable (only name and active may change)';
    end if;
    return new;
end;
$$;

create trigger dimension_identity_immutable
    before update on dimension
    for each row execute function forbid_dimension_identity_change();
create trigger dimension_no_delete
    before delete on dimension
    for each row execute function forbid_ledger_mutation();
create trigger dimension_no_truncate
    before truncate on dimension
    for each statement execute function forbid_ledger_mutation();

grant select, insert on dimension to regnmed_app;
grant update (name, active) on dimension to regnmed_app;

alter table entry add column avdeling_id uuid references dimension (id);
alter table entry add column prosjekt_id uuid references dimension (id);

-- Invoice lines carry the codes as written, so a kreditnota reverses
-- revenue on the same avdeling/prosjekt as the original line.
alter table invoice_line add column avdeling text;
alter table invoice_line add column prosjekt text;
