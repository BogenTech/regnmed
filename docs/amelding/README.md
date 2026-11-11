# A-meldingen — vendored skjema

| Fil | Versjon | Kilde | SHA-256 |
| --- | --- | --- | --- |
| `a-meldingen_v2_3.xsd` | v2.3, utgave 2024-08-27 (namespace `urn:ske:fastsetting:innsamling:a-meldingen:v2_3`) | <https://www.skatteetaten.no/contentassets/e72315197e7a4e49b000a6e33fda1642/20240827-a-meldingen_v2_3.xsd> | `6eb983eb4e82f1407859a167850f137bae285c3bf005e30c21ae8b7eea640fe2` |

Hentet 2026-11-11. Fila står under «Gjeldende dokumentasjon» på
Skatteetatens side [A-meldingen – for sluttbrukersystemer](https://www.skatteetaten.no/samarbeidspartnere/sluttbrukersystemer/a-meldingen-sbs/);
api-dokumentasjon-katalogen (`skatteetaten.github.io/api-dokumentasjon`,
sidene `innrapportering-amelding` og `innrapportering-amelding-filopplasting`)
peker på samme informasjonsmodell, `Informasjonsmodell_A-meldingen_V2_3`.
Katalogen har ingen egen XSD.

Kodene renderen bruker er sjekket mot disse, som IKKE er vendored (regneark,
oppdateres to ganger i året — R1/R2):

- Kodeliste R1 2026 — `kodelister/ekstern-kodeliste-r1-2026-2.xlsx`
- Inntektskombinasjoner R1 2026 — `inntektskombinasjoner/2026-r1-inntektskombinasjoner-ekstern_2-2.xlsx`
- Forretningsregler R1 2026 — `forretningsregler/forretningsregler-r1-2026.xlsx`

(alle under samme `contentassets/e72315197e7a4e49b000a6e33fda1642/`).

Renderen og hva den nekter: docs/lonn.md, «A-meldingen».
