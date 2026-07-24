-- Produktregister og enkelt varelager (docs/produkter.md, #39).
--
-- The product register is EDITABLE master data: documents copy the
-- values at issue time (description, price, VAT, konto live on the
-- line), so changing the register never changes an issued document.
-- The nummer is the permanent identity (immutable via trigger, like a
-- dimension code); products are never deleted, only deactivated —
-- issued lines and movements reference them forever.
--
-- Beholdning is NEVER stored: it is SUM(antall_milli) over the
-- insert-only movement log, exactly like account balances. Valuation
-- (gjennomsnittsmetoden) is a pure fold over the same log
-- (regnmed-core::lager).

create table product (
    id            uuid primary key,
    company_id    uuid not null references company (id),
    nummer        text not null check (nummer <> ''),
    navn          text not null check (navn <> ''),
    salgspris_ore bigint not null check (salgspris_ore >= 0),
    vat_code      text references vat_code (code),
    -- Inntektskonto the line defaults to (account NUMBER, not id — the
    -- register must survive kontoplan edits; resolution happens at issue).
    konto         text not null default '3000' check (konto <> ''),
    aktiv         boolean not null default true,
    -- Opt-in enkelt varelager: only lagerførte products get movements.
    lagerfort     boolean not null default false,
    created_at    timestamptz not null default now(),
    updated_at    timestamptz not null default now(),
    unique (company_id, nummer)
);

create function forbid_product_identity_change() returns trigger
language plpgsql as $$
begin
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.nummer is distinct from old.nummer
       or new.created_at is distinct from old.created_at then
        raise exception 'product identity is immutable (nummer never changes)';
    end if;
    return new;
end;
$$;

create trigger product_identity_immutable
    before update on product
    for each row execute function forbid_product_identity_change();
create trigger product_no_delete
    before delete on product
    for each row execute function forbid_ledger_mutation();
create trigger product_no_truncate
    before truncate on product
    for each statement execute function forbid_ledger_mutation();

grant select, insert on product to regnmed_app;
grant update (navn, salgspris_ore, vat_code, konto, aktiv, lagerfort, updated_at)
    on product to regnmed_app;

-- The movement log. Signed milli-units (1000 = one unit), matching
-- invoice line quantities:
--   kjop      > 0, carries anskaffelseskost per unit
--   salg      auto-inserted when an invoice line with a lagerført
--             product is issued (kreditnota lines return stock, so the
--             sign follows the negated line quantity)
--   justering manual correction / varetelling, note required
create table inventory_movement (
    id           uuid primary key,
    company_id   uuid not null references company (id),
    product_id   uuid not null references product (id),
    dato         date not null,
    kind         text not null check (kind in ('kjop', 'salg', 'justering')),
    antall_milli bigint not null check (antall_milli <> 0),
    -- Anskaffelseskost per unit for inbound movements; NULL means
    -- "at current gjennomsnittskost" (varetelling opp, returer).
    kostpris_ore bigint check (kostpris_ore is null or kostpris_ore >= 0),
    note         text,
    invoice_id   uuid references invoice (id),
    created_by   text not null check (created_by <> ''),
    created_at   timestamptz not null default now(),
    check (kind <> 'kjop' or antall_milli > 0),
    check (kind <> 'justering' or (note is not null and note <> '')),
    check (kind = 'salg' or invoice_id is null)
);

create index inventory_movement_product_idx
    on inventory_movement (product_id, dato, created_at);

create trigger inventory_movement_append_only
    before update or delete on inventory_movement
    for each row execute function forbid_ledger_mutation();
create trigger inventory_movement_no_truncate
    before truncate on inventory_movement
    for each statement execute function forbid_ledger_mutation();

grant select, insert on inventory_movement to regnmed_app;

-- Document lines REFERENCE the product but keep their own copy of the
-- values — the reference is for lager and traceability, never a lookup.
alter table invoice_line add column product_id uuid references product (id);
alter table salgsdokument_line add column product_id uuid references product (id);
alter table invoice_template_line add column product_id uuid references product (id);
