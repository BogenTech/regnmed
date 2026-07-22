-- MVA end-to-end: complete the standard VAT code list (Skatteetaten's
-- SAF-T standard tax codes, docs/saft/Standard_Tax_Codes.csv), classify
-- every code by rate class, and introduce dated VAT rates so beregning
-- and reporting always use the rate valid on the voucher date.
-- vat_code.rate_percent stays as the informational "current rate".

alter table vat_code add column rate_class text
    check (rate_class in ('regular', 'middle', 'low', 'raw_fish', 'zero'));

insert into vat_code (code, description, rate_percent) values
    ('12', 'Fradragsberettiget innenlands inngående mva, råfisk', 11.11),
    ('14', 'Fradragsberettiget innførselsmerverdiavgift, alminnelig sats', 25),
    ('15', 'Fradragsberettiget innførselsmerverdiavgift, redusert sats, næringsmidler', 15),
    ('20', 'Kostnad ved innførsel av varer, ingen mva-behandling', 0),
    ('21', 'Kostnad ved innførsel av varer, alminnelig sats', 25),
    ('22', 'Kostnad ved innførsel av varer, redusert sats, næringsmidler', 15),
    ('32', 'Utgående mva, råfisk', 11.11),
    ('51', 'Innenlandsk omsetning med omvendt avgiftsplikt, nullsats', 0),
    ('81', 'Grunnlag innførsel av varer med fradragsrett for innførselsmva, alminnelig sats', 25),
    ('82', 'Grunnlag innførsel av varer uten fradragsrett for innførselsmva, alminnelig sats', 25),
    ('83', 'Grunnlag innførsel av varer med fradragsrett for innførselsmva, redusert sats', 15),
    ('84', 'Grunnlag innførsel av varer uten fradragsrett for innførselsmva, redusert sats', 15),
    ('85', 'Grunnlag innførsel av varer som det ikke skal beregnes mva av', 0),
    ('86', 'Tjenester kjøpt fra utlandet med fradragsrett for mva, alminnelig sats', 25),
    ('87', 'Tjenester kjøpt fra utlandet uten fradragsrett for mva, alminnelig sats', 25),
    ('88', 'Tjenester kjøpt fra utlandet med fradragsrett for mva, lav sats', 12),
    ('89', 'Tjenester kjøpt fra utlandet uten fradragsrett for mva, lav sats', 12),
    ('91', 'Kjøp av klimakvoter eller gull med fradragsrett for mva', 25),
    ('92', 'Kjøp av klimakvoter eller gull uten fradragsrett for mva', 25);

update vat_code set rate_class = case
    when code in ('1', '3', '14', '21', '81', '82', '86', '87', '91', '92') then 'regular'
    when code in ('11', '15', '22', '31', '83', '84') then 'middle'
    when code in ('13', '33', '88', '89') then 'low'
    when code in ('12', '32') then 'raw_fish'
    else 'zero'
end;

alter table vat_code alter column rate_class set not null;

-- Dated rates in basis points (25 % = 2500) — integer math only, like all
-- money in regnmed. History starts 2016-01-01; automatic beregning for
-- older vouchers is out of scope.
create table vat_rate (
    rate_class text not null,
    valid_from date not null,
    rate_bp    integer not null check (rate_bp >= 0),
    primary key (rate_class, valid_from)
);

insert into vat_rate (rate_class, valid_from, rate_bp) values
    ('regular',  '2016-01-01', 2500),
    ('middle',   '2016-01-01', 1500),
    ('low',      '2016-01-01', 1000),
    ('low',      '2018-01-01', 1200),
    ('low',      '2020-04-01',  600),  -- covid-19 temporary reduction
    ('low',      '2021-10-01', 1200),
    ('raw_fish', '2016-01-01', 1111),
    ('zero',     '2016-01-01',    0);

grant select on vat_rate to regnmed_app;
