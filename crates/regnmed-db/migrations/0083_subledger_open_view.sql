-- The open remainder of every party entry, in ONE place. Until now the
-- formula (amount minus what reskontro_match has consumed, where a match
-- counts positive on its entry_a side and negative on its entry_b side)
-- was written out in each query that needed it. New queries read this
-- view; the older ones can move to it as they are touched.
create view subledger_entry_open as
select e.id as entry_id,
       e.party_id,
       e.account_id,
       e.voucher_id,
       e.amount_ore,
       e.amount_ore
         - coalesce((select sum(m.amount_ore) from reskontro_match m where m.entry_a = e.id), 0)
         + coalesce((select sum(m.amount_ore) from reskontro_match m where m.entry_b = e.id), 0)
         as remaining_ore
from entry e
where e.party_id is not null;

grant select on subledger_entry_open to regnmed_app;
