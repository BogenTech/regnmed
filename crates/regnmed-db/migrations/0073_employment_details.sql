-- The arbeidsforhold facts the a-melding requires (F290/F291) and the
-- payroll model did not hold (docs/lonn.md, «Hva modellen IKKE har»).
-- All nullable: employees registered before this migration have none of
-- them, and the renderer refuses a missing field by name rather than
-- guessing one. Editable like the rest of the employee's terms; the
-- identity stays guarded by employee_guard (0035).
alter table employee
    -- The underenhet in Enhetsregisteret the employee works at (F290),
    -- not the legal entity's own number.
    add column business_unit_orgnumber text
        check (business_unit_orgnumber ~ '^[0-9]{9}$'),
    -- Seven-digit STYRK-08 code from SSB's yrkeskatalog (F115B).
    add column occupation_code text
        check (occupation_code ~ '^[0-9]{7}$'),
    -- Basis points: 100 % = 10000.
    add column position_percent_bp integer
        check (position_percent_bp between 0 and 10000),
    -- Hours per week a full position equals, in hundredths (37,5 = 3750).
    add column full_time_hours_per_week_hundredths integer
        check (full_time_hours_per_week_hundredths between 1 and 16800),
    -- Kodeliste values (arbeidstidsordning, formForAnsettelse). The lists
    -- change per half-year, so they are checked against
    -- regnmed-core::amelding in the application, not frozen here.
    add column working_hours_scheme text check (working_hours_scheme <> ''),
    add column employment_form text check (employment_form <> ''),
    add column last_salary_change date;

grant update (business_unit_orgnumber, occupation_code, position_percent_bp,
              full_time_hours_per_week_hundredths, working_hours_scheme,
              employment_form, last_salary_change)
    on employee to regnmed_app;

-- Fastlønn or timelønn: the run knows which, and the a-melding reports
-- them under different descriptions. Null on lines from before this
-- migration, and when the run could not tell (an explicit amount for an
-- employee with neither or both wage forms) — never inferred afterwards.
alter table payroll_line
    add column pay_kind text check (pay_kind in ('fixed', 'hourly'));
