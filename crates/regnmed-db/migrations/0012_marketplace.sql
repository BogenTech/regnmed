-- Marketplace: record where an autorisasjon verification came from.
-- firm.kind and firm.autorisasjon_verified_at exist since migration 0005;
-- the ref names the register/source that confirmed it (revisjon trail).

alter table firm add column autorisasjon_ref text;

grant update (autorisasjon_verified_at, autorisasjon_ref) on firm to regnmed_app;
