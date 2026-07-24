-- Kontaktinfo for salgsdokumenter og utsendelse (docs/faktura.md, #32).
--
-- The invoice PDF needs the seller's address, kontonummer and
-- selskapsform ("Foretaksregisteret" is mandatory on salgsdokument for
-- foretak registrert der), and delivery needs the customer's e-mail.
-- All of it is editable master data — none of these columns is part of
-- any hash; the PDF stored at issue time is the evidence of what was
-- sent, whatever the registry says later.

alter table company add column address text;
alter table company add column bank_account text;
alter table company add column orgform text;
grant update (address, bank_account, orgform) on company to regnmed_app;

alter table party add column address text;
alter table party add column email text;
grant update (address, email) on party to regnmed_app;
