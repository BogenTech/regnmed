-- The buyer's own reference ("Deres referanse", EN 16931 BT-10) (#221).
-- Until now the EHF carried OUR customer number in BT-10: valid against
-- PEPPOL-EN16931-R003, but public and larger private buyers route an
-- incoming invoice on their own reference, so ours landed in their
-- exception queue.
--
-- invoice.buyer_reference is part of the issued document: set at insert,
-- covered by the existing grants (select, insert only), never updated.
-- party.buyer_reference is the customer's standing reference, used when
-- an invoice does not name one -- editable master data like the address.
-- A recurring template and a tilbud/ordre carry their own, because a
-- buyer often issues one purchase order per contract: the reference
-- belongs where the agreement lives, and the invoice copies it.
--
-- 70 characters: the reference is printed on one line of the stored PDF
-- and is what the buyer's system matches on (settings.rs, the same limit).
alter table invoice add column buyer_reference text
    check (buyer_reference <> '' and char_length(buyer_reference) <= 70);
alter table party add column buyer_reference text
    check (buyer_reference <> '' and char_length(buyer_reference) <= 70);
grant update (buyer_reference) on party to regnmed_app;
alter table invoice_template add column buyer_reference text
    check (buyer_reference <> '' and char_length(buyer_reference) <= 70);
alter table salgsdokument add column buyer_reference text
    check (buyer_reference <> '' and char_length(buyer_reference) <= 70);
