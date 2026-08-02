-- Prosjekt knyttet til kunde (#80, docs/dimensjoner.md).
--
-- Timene føres på prosjekt (#38) og prosjektet er en dimensjon (#37),
-- men registeret visste ikke HVEM prosjektet er for — «alle timene hos
-- kunde X» kunne ikke besvares uten navnegjetting, og
-- prosjektlønnsomheten (#71) trenger samme kobling per kunde.
--
-- Koblingen er METADATA på linje med navnet: redigerbar, aldri del av
-- kjeden (hashen dekker KODEN, ikke raden), og derfor utenfor
-- identitetstriggeren fra 0018. Én kunde per prosjekt er modellen — et
-- prosjekt for flere kunder er to prosjekter.

-- The FK is composite so the customer must live in the SAME company as
-- the project — a plain party(id) reference would let one company's
-- project point at another company's customer.
alter table party add constraint party_company_id_uq unique (company_id, id);

alter table dimension add column party_id uuid;
alter table dimension add constraint dimension_party_same_company
    foreign key (company_id, party_id) references party (company_id, id);

-- Bare prosjekter har kunder; en avdeling med kunde er en modellfeil.
-- At parten faktisk er en KUNDE (kind='kunde') håndheves i koden ved
-- oppslaget — en delvis unik indeks kan ikke være FK-mål.
alter table dimension add constraint dimension_party_prosjekt_only
    check (party_id is null or kind = 'prosjekt');

grant update (party_id) on dimension to regnmed_app;
