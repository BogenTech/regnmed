-- An a-melding for a month WITHOUT a payroll run (docs/lonn.md, F108):
-- every active arbeidsforhold is reported each month, paid or not, so a
-- month nobody was paid still needs a melding -- and it has no run to
-- hang from. Such a submission is identified by its company and period
-- alone.
alter table amelding_submission alter column run_id drop not null;

create index amelding_submission_month_idx
    on amelding_submission (company_id, period, created_at desc)
    where run_id is null;
