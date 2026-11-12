-- Frikort with a beløpsgrense and skattekort spread over 12 months
-- (docs/lonn.md «Skattekort fra Skatteetaten»).
--
-- Both were kept with a note until now, because payroll assumed every
-- card was spread over 10,5 months (half trekk in December, none on
-- feriepenger) and had no frikort limit to count against. The employee
-- row gets what the calculation needs; the stored VALUE stays Norwegian
-- ('frikort'), like 'prosent' and 'tabell'.
alter table employee drop constraint employee_trekk_type_check;
alter table employee add constraint employee_trekk_type_check
    check (trekk_type in ('prosent', 'tabell', 'ingen', 'frikort'));
alter table employee
    -- A percentage card spread over 12 months (kildeskatt, among others):
    -- trekk on feriepenger and in full in December. Tables carry this in
    -- their number and do not use the column.
    add column withholding_twelve_months boolean not null default false,
    -- The frikort's beløpsgrense allotted to THIS employer, in øre.
    add column tax_free_card_limit_ore bigint check (tax_free_card_limit_ore >= 0),
    add constraint employee_tax_free_card_check
        check (trekk_type <> 'frikort' or tax_free_card_limit_ore is not null);
grant update (withholding_twelve_months, tax_free_card_limit_ore) on employee to regnmed_app;

alter table tax_card drop constraint tax_card_applied_check;
alter table tax_card add constraint tax_card_applied_check
    check (applied in ('prosent', 'tabell', 'ingen', 'frikort'));
alter table tax_card
    add column applied_twelve_months boolean not null default false,
    add column applied_tax_free_card_limit_ore bigint;

-- How the trekk was computed («35,00 %», «tabell 8000», «frikort, 50 %
-- av … over beløpsgrensen»), written with the line so the payslip says
-- what was done THEN, not what the employee's card says today. NULL on
-- lines from before this migration.
alter table payroll_line add column withholding_method text;
