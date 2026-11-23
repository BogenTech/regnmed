-- An employee's holiday pay rate can now change, so each payroll line records
-- the rate it was accrued at. docs/lonn.md, "Når satsen endres".
alter table payroll_line add column holiday_pay_bp integer;

-- Lines from before this migration stay NULL because payroll lines are
-- insert-only. No path could change the rate until now, so the rate each
-- employee has today is the rate all of their earlier lines used. The app role
-- has no update grant on this column.
alter table employee add column legacy_holiday_pay_bp integer;
update employee set legacy_holiday_pay_bp = feriepenger_bp;
