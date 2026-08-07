# Balansedokumentasjon

**Bokføringsloven §11**, jf. bokføringsforskriften kap. 6: for hver
balansepost skal det foreligge dokumentasjon av saldoen ved periodeslutt
— kontoutskrift, varetellingsliste, avstemming mot innsendte oppgaver,
lånesaldo. Oppbevaringsplikt fem år (bfl. §13).

Vi hadde bankavstemming (#15), åpne poster i reskontroen og
revisjonsrapportens kontroller, men ingen struktur for selve
avstemmingen. Et selskap kunne låse en periode uten at én eneste
balansekonto var avstemt, og revisor hadde ingen annen kilde enn å be om
det på e-post.

## 1. Modellen

`balanse_dokumentasjon` er **innsettings-bar**, etter periodelåsens og
attesteringens doktrine: (selskap, konto, periode) med lagret saldo,
forklaring, hvem som avstemte og når, og et valgfritt vedlegg med
SHA-256.

- **Ingen retting.** En ny avstemming er en NY RAD, og den nyeste
  gjelder. En avstemming som kunne skrives om i ettertid er ikke
  dokumentasjon, den er en påstand.
- **Vedlegget ER dokumentasjonen** når det finnes. Innholdet er
  uforanderlig som bilagsvedleggene, og hashen sjekkes ved nedlasting:
  det som kommer ut er bevist å være det som gikk inn.
- **Saldoen leses av systemet**, ikke oppgitt av kalleren — ellers ville
  kontrollen vært en egenmelding.

## 2. Hvorfor saldoen lagres

Fordi avviket skal være synlig.

Er kontoen bokført videre etter at den ble avstemt, sier rapporten det:
`avvik_ore` = bokført saldo nå minus saldoen som ble dokumentert. Et
øyeblikksbilde som stille fulgte hovedboken ville skjult nøyaktig den
forskjellen avstemmingen finnes for å fange — et etterslept bilag inn i
en periode noen allerede har sagt god for.

Det er **ikke det samme som udokumentert**, og rapporten blander dem
ikke: avstemmingen skjedde, saldoen flyttet seg etterpå. Portalen sier
«bokført X etter avstemming» med egen farge.

## 3. Kontrollen er et AVVIK

Revisjonsrapporten fikk kontroll «Balansedokumentasjon»
(docs/revisjon.md): hvilke balansekontoer med saldo ≠ 0 mangler
dokumentasjon for perioden.

Forskjellen fra kontroll «Dokumentasjon» (#85), som er
INFORMASJONSKONTROLL, ligger i hva loven ber om:

| | §10 (bilagsdokumentasjon) | §11 (balansedokumentasjon) |
| --- | --- | --- |
| Krav | bokførte opplysninger skal være dokumentert | dokumentasjonen SKAL foreligge for balanseposten |
| Kan ligge annet sted? | ja, lovlig i annet oppbevaringsmedium | regnmed er der selskapet registrerer at den finnes |
| I rapporten | informasjon | **avvik** |

Målt ved **siste låste periode**. Uten en låst periode er det ingenting
å dokumentere ennå, og det sier kontrollen — den dikter ikke opp en
frist.

Kontoer som ender perioden på null er utelatt: det er ingenting å
dokumentere om en saldo på ingenting, og å liste dem ville begravd de
kontoene som betyr noe.

## 4. Web API

| Endepunkt | Rett |
| --- | --- |
| `GET /companies/{id}/balansedokumentasjon?periode=` | `RAPPORT_LES` |
| `POST /companies/{id}/balansedokumentasjon` | `BILAG_BOKFOR` |
| `POST …/balansedokumentasjon/vedlegg?konto=&periode=&forklaring=&filename=` | `BILAG_BOKFOR` |
| `GET …/balansedokumentasjon/{id}/vedlegg` | `RAPPORT_LES` |
| `GET …/balansedokumentasjon/historikk?konto=&periode=` | `RAPPORT_LES` |

Lesing er `RAPPORT_LES` fordi dette ER en rapport, og revisor må kunne
lese den gjennom et lesende oppdrag. Å registrere en avstemming er
`BILAG_BOKFOR`: det er regnskapsførerens påstand om hva en balansepost
består av, ikke noe en leser gjør.

Uten `?periode` brukes siste låste periode — den samme
revisjonsrapporten måler, så de to kan ikke være uenige ved et uhell.

Portalen: fane **Balansedokumentasjon** under Rapporter.

## 5. Avgrensning

**v1 sperrer IKKE periodelåsing på manglende avstemming.** Kontrollen
rapporterer. En sperre er en policy-avgjørelse på linje med
attesteringspolicyen (#47) og kan legges oppå senere — `target_kind`-
mønsteret der gjør det til en migrasjon, ikke en omskriving.

## 6. Hvor det er testet

`crates/regnmed-db/tests/balansedok.rs`: at bare balansekontoer med
saldo listes (en resultatkonto avvises høylytt), at saldoen lagres slik
den var, at bokføring ETTER avstemmingen dukker opp som differanse, at
en ny avstemming er en ny rad med den gamle i behold, og at vedlegget
kommer tilbake byte for byte med hashen sjekket.

`crates/regnmed-api/tests/grupper/revisjon.rs`: den friske hovedboken
må nå dokumentere balansekontoen sin for at rapporten skal være grønn —
kontrollen er sett feile ved å avstemme feil konto.
