-- Betalingsoppfølging (docs/purring.md): purrehistorikken per faktura.
--
-- "Forfalt" er alltid beregnet (due_date + åpen reskontropost), aldri
-- lagret tilstand — bare selve purreskrittene er rader, og de er bevis:
-- insert-only, med det rendrede dokumentet lagret slik at kravet kan
-- gjenutstedes byte for byte for alltid, uansett hvordan satsene endrer
-- seg senere. Gebyr og rente som kreves bokføres som ordinære bilag i
-- samme transaksjon (voucher_id) — aldri sidestilte gebyrer.

create table invoice_reminder (
    id            uuid primary key,
    invoice_id    uuid not null references invoice (id),
    steg          text not null check (steg in ('paminnelse', 'purring', 'inkassovarsel')),
    sent_date     date not null,
    frist_date    date not null,
    -- Utestående på fakturaen da skrittet ble registrert (øyeblikksbilde).
    remaining_ore bigint not null,
    -- 0 når ikke krevd; > 0 betyr at voucher_id bærer bokføringen.
    gebyr_ore     bigint not null check (gebyr_ore >= 0),
    rente_ore     bigint not null check (rente_ore >= 0),
    voucher_id    uuid references voucher (id),
    document      text not null check (document <> ''),
    created_by    text not null check (created_by <> ''),
    created_at    timestamptz not null default now(),
    check ((gebyr_ore + rente_ore > 0) = (voucher_id is not null))
);

create index invoice_reminder_invoice on invoice_reminder (invoice_id);

create trigger invoice_reminder_append_only
    before update or delete on invoice_reminder
    for each row execute function forbid_ledger_mutation();
create trigger invoice_reminder_no_truncate
    before truncate on invoice_reminder
    for each statement execute function forbid_ledger_mutation();

grant select, insert on invoice_reminder to regnmed_app;
