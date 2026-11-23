-- The operator's statement that a holiday pay payout is exempt, on death or
-- as regular payment with wages (skattebetalingsforskriften § 5-7-1 (1) c, d).
-- It removes withholding, so it must be answerable afterwards.
alter table payroll_line
    add column holiday_pay_exempt boolean not null default false;

-- Holiday pay outside the exemption takes the percentage printed on a table
-- card (§ 5-10-3), which earlier fetches discarded. The stored card is the
-- evidence, so employees whose table came from a card need no new fetch.
update employee e
   set trekk_prosent_bp = latest.bp::integer
  from (
      select distinct on (c.employee_id)
             c.employee_id,
             c.applied_table,
             (select case
                         when jsonb_typeof(t -> 'trekktabell' -> 'prosentsats') = 'number'
                         then (t -> 'trekktabell' ->> 'prosentsats')::numeric * 100
                     end
                from jsonb_array_elements(
                         case jsonb_typeof(c.card -> 'skattekort' -> 'forskuddstrekk')
                             when 'array' then c.card -> 'skattekort' -> 'forskuddstrekk'
                             else '[]'::jsonb
                         end
                     ) t
               where t ->> 'trekkode' = 'loennFraHovedarbeidsgiver'
               limit 1) as bp
        from tax_card c
       where c.applied = 'tabell'
       order by c.employee_id, c.created_at desc
  ) latest
 where e.id = latest.employee_id
   and e.trekk_type = 'tabell'
   and e.trekk_tabell = latest.applied_table
   and e.trekk_prosent_bp is null
   and latest.bp between 0 and 10000
   and latest.bp = trunc(latest.bp);
