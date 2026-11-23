-- Ferieloven § 10 nr. 3: the 2,3-point addition for employees over 60 is not
-- paid on the part of the basis above 6 G. docs/lonn.md has the sources.
insert into sats (domene, valid_from, verdi, enhet, kilde) values
    -- Covers someone leaving before 1 May 2025, the first payroll year here.
    ('folketrygdens_grunnbelop', date '2024-05-01', 12402800, 'ore',
     'FOR-2024-05-24-815 — grunnbeløpet i folketrygden 124 028 kroner fra 1. mai 2024'),
    ('folketrygdens_grunnbelop', date '2025-05-01', 13016000, 'ore',
     'FOR-2025-05-23-837 § 1 — grunnbeløpet i folketrygden 130 160 kroner fra 1. mai 2025'),
    ('folketrygdens_grunnbelop', date '2026-05-01', 13654900, 'ore',
     'FOR-2026-05-22-869 § 1 — grunnbeløpet i folketrygden 136 549 kroner fra 1. mai 2026'),
    -- The rate is unchanged. The register is append-only, so this row is what
    -- corrects the 0035 source, which named the wrong year.
    ('feriepenger_over_60', date '2026-11-23', 1250, 'bp',
     'ferieloven § 10 nr. 3 — 10,2 % + 2,3 prosentpoeng, tillegget bare opp til 6 G, fra opptjeningsåret før ferieåret arbeidstakeren fyller 60');

-- How much of the addition the cap removed in this line. The next run needs
-- it to reach the year's cut exactly when G changes during the year, without
-- touching lines already posted.
alter table payroll_line
    add column holiday_pay_cap_cut_ore bigint not null default 0;
