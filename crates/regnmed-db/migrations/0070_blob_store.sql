-- Documents move out of Postgres into object storage (docs/blob-store.md).
--
-- A document's bytes are stored under their SHA-256 in an object store;
-- the row keeps the hash and the metadata, and its content column is NULL.
-- The hash was already stored and checked on every read, so a tampered or
-- missing object is detected exactly as altered bytes in the database were.
-- Without an object store configured (local development, tests) the bytes
-- stay in the content column as before.
--
-- Six columns: attachment.content, inbox_document.content,
-- inbox_mail.raw, inbox_mail_attachment.content, expense.receipt_content,
-- balanse_dokumentasjon.vedlegg. (payment_run.file — a few KB of pain.001
-- XML on a mutable run row — stays.)
--
-- Moving EXISTING bytes out needs an update, which the append-only guards
-- forbid. They get one exception, the same shape as the timesheet's
-- billing marker: the content column may go from bytes to NULL, provided
-- every other column is unchanged and the stored hash IS the hash of the
-- bytes being removed (or, where no hash was stored, is set to it in the
-- same update). Nothing else about a row can change, and the content can
-- never come back or be replaced.

create function is_content_offload(o jsonb, n jsonb, c text, h text) returns boolean
language sql immutable as $$
    select (o - c - h) = (n - c - h)
       and o ->> c is not null
       and n ->> c is null
       and n ->> h = '\x' || encode(sha256(decode(substr(o ->> c, 3), 'hex')), 'hex')
       and (o ->> h is null or o ->> h = n ->> h)
$$;

-- Tables guarded by forbid_ledger_mutation on UPDATE: deletes stay
-- forbidden; an update is allowed only as an offload.
create function guard_content_offload() returns trigger
language plpgsql as $$
begin
    if is_content_offload(to_jsonb(old), to_jsonb(new), tg_argv[0], tg_argv[1]) then
        return new;
    end if;
    raise exception 'ledger rows are append-only (% on %)', tg_op, tg_table_name;
end;
$$;

-- attachment
alter table attachment alter column content drop not null;
drop trigger attachment_append_only on attachment;
create trigger attachment_no_delete
    before delete on attachment
    for each row execute function forbid_ledger_mutation();
create trigger attachment_offload_only
    before update on attachment
    for each row execute function guard_content_offload('content', 'sha256');

-- inbox_mail_attachment
alter table inbox_mail_attachment alter column content drop not null;
drop trigger inbox_mail_attachment_append_only on inbox_mail_attachment;
create trigger inbox_mail_attachment_no_delete
    before delete on inbox_mail_attachment
    for each row execute function forbid_ledger_mutation();
create trigger inbox_mail_attachment_offload_only
    before update on inbox_mail_attachment
    for each row execute function guard_content_offload('content', 'sha256');

-- balanse_dokumentasjon: the attachment is optional, so "has an attachment"
-- now means a hash and a name, with or without bytes in the row.
alter table balanse_dokumentasjon drop constraint balanse_dokumentasjon_check;
alter table balanse_dokumentasjon drop constraint balanse_dokumentasjon_check1;
alter table balanse_dokumentasjon add constraint balanse_dokumentasjon_vedlegg_check
    check ((vedlegg_sha256 is null) = (vedlegg_navn is null)
           and (vedlegg is null or vedlegg_sha256 is not null));
drop trigger balanse_dokumentasjon_append_only on balanse_dokumentasjon;
create trigger balanse_dokumentasjon_no_delete
    before delete on balanse_dokumentasjon
    for each row execute function forbid_ledger_mutation();
create trigger balanse_dokumentasjon_offload_only
    before update on balanse_dokumentasjon
    for each row execute function guard_content_offload('vedlegg', 'vedlegg_sha256');

-- inbox_document: its own guard allows the decision columns to change;
-- the offload is added in front of it.
alter table inbox_document alter column content drop not null;
create or replace function guard_inbox_update() returns trigger
language plpgsql as $$
begin
    if is_content_offload(to_jsonb(old), to_jsonb(new), 'content', 'sha256') then
        return new;
    end if;
    if old.status <> 'ny' then
        raise exception 'inbox document % is already decided (%)', old.id, old.status;
    end if;
    if new.content is distinct from old.content
       or new.sha256 is distinct from old.sha256
       or new.filename is distinct from old.filename
       or new.content_type is distinct from old.content_type
       or new.byte_size is distinct from old.byte_size
       or new.company_id is distinct from old.company_id
       or new.uploaded_by is distinct from old.uploaded_by
       or new.created_at is distinct from old.created_at
       or new.inbox_mail_id is distinct from old.inbox_mail_id then
        raise exception 'inbox document content is immutable';
    end if;
    return new;
end;
$$;

-- expense: a receipt is now "a hash, a name and a type"; the bytes may be
-- in the row or in the object store.
alter table expense drop constraint expense_check;
alter table expense drop constraint expense_check2;
alter table expense add constraint expense_receipt_check
    check (kind <> 'utlegg' or (receipt_sha256 is not null
                                and receipt_filename is not null and receipt_content_type is not null));
alter table expense add constraint expense_no_receipt_check
    check (kind = 'utlegg' or (receipt_content is null and receipt_sha256 is null));
create or replace function expense_guard() returns trigger
language plpgsql as $$
begin
    if is_content_offload(to_jsonb(old), to_jsonb(new), 'receipt_content', 'receipt_sha256') then
        return new;
    end if;
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.person_id is distinct from old.person_id
       or new.kind is distinct from old.kind
       or new.dato is distinct from old.dato
       or new.beskrivelse is distinct from old.beskrivelse
       or new.belop_ore is distinct from old.belop_ore
       or new.receipt_filename is distinct from old.receipt_filename
       or new.receipt_content_type is distinct from old.receipt_content_type
       or new.receipt_content is distinct from old.receipt_content
       or new.receipt_sha256 is distinct from old.receipt_sha256
       or new.km is distinct from old.km
       or new.sats_ore_per_km is distinct from old.sats_ore_per_km
       or new.trekkfri_ore is distinct from old.trekkfri_ore
       or new.trekkpliktig_ore is distinct from old.trekkpliktig_ore
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'expense content is immutable from submission';
    end if;
    if old.status = 'innsendt' and new.status in ('godkjent', 'avvist') then
        return new;
    end if;
    if old.status = 'godkjent' and new.status = 'utbetalt'
       and new.decided_by = old.decided_by and new.decided_at = old.decided_at
       and new.voucher_id = old.voucher_id and new.motkonto = old.motkonto then
        return new;
    end if;
    raise exception 'expense % kan ikke gå fra % til %', old.id, old.status, new.status;
end;
$$;

-- inbox_mail: the raw message never had a stored hash. New rows carry one
-- (and no bytes when an object store is configured); an offload of an old
-- row sets it to the hash of the bytes it removes.
alter table inbox_mail alter column raw drop not null;
alter table inbox_mail add column raw_sha256 bytea
    check (raw_sha256 is null or octet_length(raw_sha256) = 32);
alter table inbox_mail add constraint inbox_mail_raw_check
    check (raw is not null or raw_sha256 is not null);
create or replace function inbox_mail_guard() returns trigger
language plpgsql as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'mottatt e-post slettes ikke';
    end if;
    if is_content_offload(to_jsonb(old), to_jsonb(new), 'raw', 'raw_sha256') then
        return new;
    end if;
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.message_id is distinct from old.message_id
       or new.from_address is distinct from old.from_address
       or new.subject is distinct from old.subject
       or new.body is distinct from old.body
       or new.raw is distinct from old.raw
       or new.raw_sha256 is distinct from old.raw_sha256
       or new.received_at is distinct from old.received_at then
        raise exception 'mottatt e-post er uforanderlig';
    end if;
    if old.status <> 'karantene' then
        raise exception 'e-posten er allerede %', old.status;
    end if;
    if new.status not in ('mottatt', 'avvist') then
        raise exception 'ugyldig utfall %', new.status;
    end if;
    return new;
end;
$$;
