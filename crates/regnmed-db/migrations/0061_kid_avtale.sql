-- KID belongs to a bank agreement, not to an invoice number (#104).
--
-- regnmed computed a KID for every invoice and printed it, which is an
-- instruction to the payer about how to mark the payment. A company
-- without an OCR/KID agreement cannot receive one: the payer's bank
-- either refuses the KID or does not offer it for that account, and the
-- invoice then asks for something that cannot be done.
--
-- Default FALSE on purpose, existing companies included: we do not
-- claim a bank agreement we have no evidence for, the same rule
-- company_registrering follows for the mva påtegning (#81, #99). A
-- company that has the agreement ticks it in Firmaopplysninger, and
-- invoices issued from then on carry the KID.
--
-- invoice.kid keeps being computed and stored regardless — OCR import
-- resolves a payment to its invoice BY KID (#7), so the value must
-- exist even when the document does not show it.
alter table company add column kid_avtale boolean not null default false;

comment on column company.kid_avtale is
    'Har selskapet KID-/OCR-avtale med banken? Styrer om KID trykkes på
     salgsdokumenter og purringer, og om EHF-en bærer cbc:PaymentID.';
