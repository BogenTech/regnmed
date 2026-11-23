-- Skattebetalingsforskriften § 5-6-1 (1) a: holiday pay paid in the earning
-- year because the employee leaves is withheld on. This records how much of
-- a line's payout was that, so a later payment in the same year cannot count
-- the same earned holiday pay twice. docs/lonn.md has the rule.
alter table payroll_line
    add column holiday_pay_earning_year_ore bigint not null default 0;
