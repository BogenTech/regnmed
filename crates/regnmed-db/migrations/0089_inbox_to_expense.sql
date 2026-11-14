-- A receipt in the bilagsinnboks can become an expense claim
-- (docs/utlegg.md «Agenten»).
--
-- Employees send receipts to the company's e-post-inn address, and they
-- land in the innboks next to the supplier invoices. A receipt someone
-- paid with their own money is not a voucher to post: it is a claim to be
-- reimbursed. Whoever sorts the innboks — a bookkeeper or an agent — turns
-- it into a DRAFT claim for that person, and the document's decision is
-- recorded one way, like 'bokfort' and 'avvist': it went to that claim.

alter table inbox_document drop constraint inbox_document_status_check;
alter table inbox_document add constraint inbox_document_status_check
    check (status in ('ny', 'bokfort', 'avvist', 'utlegg'));
alter table inbox_document
    add column expense_id uuid references expense (id),
    add constraint inbox_document_expense_check
        check ((status = 'utlegg') = (expense_id is not null));

grant update (expense_id) on inbox_document to regnmed_app;
