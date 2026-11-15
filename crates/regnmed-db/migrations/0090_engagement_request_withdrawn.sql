-- A company may withdraw a request it sent before the firm has decided.
-- Withdrawal is a decision like the firm's: the same three columns change
-- once, from 'pending', and the row stays — the request is still part of
-- the history of who asked whom.

alter table engagement_request drop constraint engagement_request_status_check;
alter table engagement_request add constraint engagement_request_status_check
    check (status in ('pending', 'accepted', 'declined', 'withdrawn'));
