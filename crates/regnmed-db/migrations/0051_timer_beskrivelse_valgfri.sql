-- Ukegridet for timeføring (docs/timer.md): en celle er timene på en dag,
-- og beskrivelsen er VALGFRI detalj — regnearkfølelsen dør om hver celle
-- krever en tekst. 0023 krevde ikke-tom beskrivelse; kravet oppheves, men
-- kolonnen forblir not null (tom tekst betyr «uten notat», aldri NULL).
alter table time_entry drop constraint time_entry_beskrivelse_check;
