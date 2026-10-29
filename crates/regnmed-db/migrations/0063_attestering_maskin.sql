-- «Alt en maskin bokfører skal et menneske ha sett» (#101, #103).
--
-- 0030 lot policyen slå inn over en BELØPSGRENSE. Det er riktig for
-- mennesker: en ansatt som bokfører en kaffekvittering trenger ingen
-- attestant. For en agent er grensen den gale aksen — det som gjør et
-- maskinbokført bilag verdt et blikk er ikke beløpet, men at ingen så
-- det. #103s doktrine er derfor at agentens STANDARD er forslagsrett,
-- og dette er innstillingen som gjør standarden håndhevbar også når
-- eieren har gitt agenten bokføringsrett.
--
-- NULL = ikke valgt, og da gjelder beløpsgrensen alene (0030-oppførselen
-- urørt for alle eksisterende rader). En policy som er AV krever
-- ingenting — flagget er ikke en egen bryter utenfor policyen, for da
-- ville det finnes to steder å slå av attestering.
--
-- Fire øyne virker allerede uten dette: en agent er sin egen person-rad,
-- så 0030s nekt mot å attestere og bokføre selv treffer en agent
-- presist. Det flagget legger til er at MENNESKET må være det andre
-- øyet — en agent kan ikke attestere en annen agents bilag.

alter table attestation_policy add column krev_for_maskin boolean;

comment on column attestation_policy.krev_for_maskin is
    'Når sann: bilag bokført av en integrasjon krever godkjent attestering '
    'av et menneske, uavhengig av belopsgrense_ore (#101).';
