-- Leveringstidspunkt og -sted på salgsdokumentet (#81).
--
-- Bokføringsforskriften §5-1-1 nr. 4 krever «tidspunktet og stedet for
-- levering av ytelsen» på ethvert salgsdokument — faktura som
-- kreditnota. Feltet har manglet siden fakturaen ble bygget, så hver
-- faktura regnmed har utstedt er mangelfull på dette punktet.
--
-- Kolonnene er NULLBARE, og det er et bevisst valg: en allerede utstedt
-- faktura har ingen registrert leveringsdato, og å backfille den med
-- fakturadatoen ville vært å DIKTE OPP et rettsfaktum på et
-- salgsdokument som ikke kan endres. Historikken forblir mangelfull og
-- synlig mangelfull; kravet håndheves i stedet ved utstedelse, der
-- `InvoiceDraft.delivery_date` ikke er valgfri. Nye fakturaer får
-- alltid en dato.
--
-- Fakturaen er innsettings-bar (0010: bare select+insert til
-- regnmed_app), så kolonnene settes ved utstedelse og kan aldri endres
-- etterpå — samme uforanderlighet som resten av dokumentet.

alter table invoice add column delivery_date  date;
alter table invoice add column delivery_place text;

comment on column invoice.delivery_date is
    'Leveringstidspunkt, bokføringsforskriften §5-1-1 nr. 4. Null bare på fakturaer utstedt før #81 — aldri på nye.';
comment on column invoice.delivery_place is
    'Leveringssted, samme hjemmel. Valgfritt: kreves «der det er relevant», typisk ved vareleveranse.';
