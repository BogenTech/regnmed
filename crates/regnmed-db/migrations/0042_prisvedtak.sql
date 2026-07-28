-- Prisvedtak 2026-07-28 (#65, docs/abonnement.md §4).
--
-- Konkurrentanalysen (verifisert mot prissidene 2026-07-28: Tripletex
-- «fra 199» men lønn krever Pro 479; Fiken 219 + 69 per modul;
-- Conta 209/339 årlig fakturert; PowerOffice 425 + 9 kr/bilag) viste
-- at markedet priser MODULENE, ikke systemet. regnmed priser det
-- eneste som faktisk koster per kunde: menneskelig support.
--
--   * basis     49 kr/mnd — alt inkludert, selvbetjent (dokumentasjon)
--   * standard  99 kr/mnd — alt inkludert + e-postsupport
--
-- Innføringsprisen på 249 (0041) gjaldt juli og ble aldri fakturert;
-- de nye radene gjelder fra 2026-08-01. En senere prisendring er en ny
-- rad — eksisterende kunder beholdes på sin plan (grandfathering er
-- gratis når prisen er daterte data).
insert into abonnement_pris (plan, pris_ore_per_mnd, valid_from, kilde)
values
    ('basis', 4900, '2026-08-01',
     'prisvedtak 2026-07-28 — alt inkludert, selvbetjent'),
    ('standard', 9900, '2026-08-01',
     'prisvedtak 2026-07-28 — alt inkludert, e-postsupport');
