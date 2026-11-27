# SAF-T Financial — Skatteetatens offisielle artefakter

Kopiert inn fra [Skatteetaten/saf-t](https://github.com/Skatteetaten/saf-t)
(første gang 2026-07-22; begge skjemaene fornyet 2026-11-14 fra commit
`b54f1cf`), så bygg og CI aldri avhenger av at GitHub svarer:

- `Norwegian_SAF-T_Financial_Schema_v_1.30.xsd` og
  `Norwegian_SAF-T_Financial_Schema_v_1.40.xsd` — de offisielle skjemaene
  eksporten valideres mot (enhetstesten `validates_against_official_xsd`,
  begge versjonene, og hver ekte eksport via `xmllint`). SHA-256:
  - 1.30: `5362d6778872b4ed2524e09b083e634ae950a4f1d9f1b95076b4c9e4df6a0650`
  - 1.40: `272d5a3eff9df07cdff15a4792e34a31ab4359e27dc3215951dd017b76b89d01`
- `About_SAF-T_Financial_schema_v.1.40.txt` og
  `Bug fix in version 1.30 and version 1.40.txt` — Skatteetatens notater om
  hva 1.40 endret, og om rettelsen 2026-10-16 som gjorde
  `SelectionCriteria` obligatorisk i begge skjemaene (uten nytt
  versjonsnummer). Filnavnene er Skatteetatens egne. Vår 1.30-kopi var fra
  før rettelsen; fornyingen tok den med, og eksporten har alltid skrevet
  `SelectionCriteria`.
- `generated_auditfile_formatted_v_1.40.xml` — Skatteetatens eksempelfil
  for 1.40; importtesten `imports_skatteetatens_1_40_example` leser den.
- `naeringsspesifikasjon_2025-2026.csv` — kodelisten for gruppering
  (standardkonto → GroupingCategory/GroupingCode). En kopi er bygget inn i
  `regnmed-core` (`src/saft/`); når Skatteetaten publiserer listen for et
  nytt inntektsår, oppdateres begge kopiene sammen.
- `Standard_Tax_Codes.csv` — standard mva-koder; regnmeds `vat_code`-tabell
  bruker disse kodene direkte, derfor kan eksporten sette
  `StandardTaxCode` = vår egen kode.
- `About_SAF-T_Financial_v.1.3.txt` — Skatteetatens versjonsnotater for 1.30.

Eksporter en fil med:

```sh
regnmed saft-export --orgnr 999888777 --year 2026 --contact "Kari Nordmann"
```

## Hvilken versjon en fil skrives i

`regnmed-core::saft::schema_version` velger ut fra utvalgets startdato:
**1.40 for en periode som starter 1. januar 2027 eller senere, ellers
1.30**. 1.40 gjelder fra første periode som starter 1. januar 2027 eller
senere og er frivillig før det; 1.30 er gyldig for 2026 og tidligere. Et
utvalg som starter i 2026 og går inn i 2027 skrives derfor som 1.30 —
startdatoen velger allerede årgangen av grupperingskodelisten på samme måte.

Alle endringene i 1.40 er bakoverkompatible, og ingen krever data vi ikke
har:

- `Owners/AccountID` kan gjentas: vi skriver ikke `Owners`.
- `CurrencyAmount` og `ExchangeRate` har fått bredere desimaltyper: våre
  verdier passer i begge versjonene.
- `VirtualCurrencyType` og `Country` i beløpsstrukturen er valgfrie: vi
  bruker dem ikke.
- `DebitNOKTaxAmount`/`CreditNOKTaxAmount` brukes bare når et beløp i
  `TaxInformation` er i en annen valuta enn NOK: våre er alltid i NOK
  (merverdiavgiften bokføres i norske kroner, bokføringsforskriften § 4-2).

Det samme dokumentet validerer altså mot begge skjemaene, og
versjonsstrengen er den eneste forskjellen. **En eksport for 2027 feiler
fortsatt i dag**, med vilje: Skatteetaten har ennå ikke publisert
grupperingslisten for næringsspesifikasjonen for 2027, og eksporten nekter
et år ingen innkopiert liste dekker (docs/regelverk.md). Å kopiere inn den
listen er et punkt i regelverksrevisjonen i desember.

Importen leser begge versjonene: den leser elementer etter navn, og
tilleggene i 1.40 er elementer den ikke leser.

Norsk SAF-T krever en kontaktperson i headeren, og det er ikke stamdata
regnmed har — derfor er `--contact` obligatorisk. Kontoer som ikke selv er
standardkontoer, grupperes under nærmeste standardkonto og listes på stderr
så en regnskapsfører kan se over dem.

## Levering ved kontroll — kontrollopplysninger-API-et (#183)

Kartlagt 2026-11-26. **API-et finnes**, men ikke i `docs/api/` i
Skatteetaten/api-dokumentasjon — det står i `docs/anvendelsesomraader/`,
og SBS-siden lenker dit. Kilder, alle lest 2026-11-26:

- [SBS-siden «Innsending av kontrollopplysninger»](https://www.skatteetaten.no/samarbeidspartnere/sluttbrukersystemer/innsending-av-kontrollopplysninger/)
- [API-dokumentasjonen](https://skatteetaten.github.io/api-dokumentasjon/anvendelsesomraader/innsending-kontrollopplysninger)
  — fanene Om tjenesten, Eksempler, Feilkoder og Test
- [OpenAPI `innrapportering-kontroll-api` 1.0.0](https://app.swaggerhub.com/apis/skatteetaten/innrapportering-kontroll-api/1.0.0)
  (publisert 2026-06-11; en upublisert 1.1.0 fra 2026-11-04 endrer bare
  svartypen på dokumentuthentingen)
- [«Bruk av API-er for innrapportering»](https://skatteetaten.github.io/api-dokumentasjon/om/bruk_innrapportering)

**Tilgang.**

| Hva | Verdi | Kilde |
| --- | --- | --- |
| Scope | `skatteetaten:innleveringkontroll` (merk: «innlevering», mens vertsnavnet sier «innrapportering») | API-dok., Om tjenesten |
| Token | Maskinporten **med systembruker** — et rent Maskinporten-token avvises med `GLD_1063` | API-dok., Feilkoder |
| Egen virksomhet | standard systembruker med ressursen `ske-kontroll` | API-dok., Om tjenesten |
| Regnskapsfører | klient-systembruker med `regnskapsforer-med-signeringsrettighet` eller `-uten-signeringsrettighet` (også godtatt: `ansvarlig-revisor`, `revisormedarbeider`, `skatt-naering`; `ordinaer-post-til-virksomheten` gir bare lesing) | API-dok., Om tjenesten |
| Miljøer | test `https://innrapporteringkontroll.api.skatteetaten-test.no/api/v1`, prod `https://innrapporteringkontroll.api.skatteetaten.no/api/v1` | OpenAPI `servers` |

Systemet vårt (`935115086_regnmed`, docs/gov.md) har allerede de to
REGN-pakkene, så regnskapsførerruten krever ingen ny registrering. Den
har også `ske-kontroll` i `rights` for egen-virksomhet-ruten (lagt til
2026-11-27, `scripts/altinn-system.py`).

**Flyten** (Eksempler + OpenAPI):

1. `POST /innsendinger` med `dialogid` og `metadata`
   (`kontaktinformasjon.navn` og `.epostadresse` påkrevd, `telefonnummer`
   og `kommentarTilFiler` valgfrie) → innsending-id, status
   `UNDER_INNSENDING`.
2. `POST /innsendinger/{id}/opplastinger`, header `filnavn`, kroppen er
   filen. SAF-T går som `application/xml`; PDF, PNG, JPEG og Office-filer
   godtas som andre vedlegg. **Zip er ikke blant typene.**
3. Valideringen er asynkron: poll `GET /innsendinger/{id}` til
   `KLAR_TIL_INNSENDING`. En fil som feiler får `VALIDERING_FEILET` med
   `feilliste` (XSD-meldingen med linje og kolonne, kode `GLD_010`).
4. `PUT /innsending` med innsending-id-en → `dialogId` + `forsendelseId`.
   Krever minst én godkjent opplasting (`GLD_1067`). Eksempelet viser en
   UUID lik `dialogid` i kroppen, men skjemaet sier `InnsendingId` — vi
   følger skjemaet og bekrefter i test.
5. Kvitteringen hentes med
   `GET /dialoger/{dialogId}/forsendelser/{forsendelseId}/dokumenter/{dokumentId}`;
   dokument-id-en kommer fra Dialogporten. Kvitteringen
   (`KontrollAvRegnskapOgKassasystem`) lister filnavn, orgnr, navn og
   periode per SAF-T-fil — Skatteetaten leser dem ut av filen selv.

**Hvordan innsendingen knyttes til kontrollen:** `dialogid` er påkrevd,
og feilkodene viser at det må være en dialog som alt finnes
(`GLD_1017` «ingen dialog med denne IDen», `GLD_1015` «ikke autorisert
for å levere på denne dialogen»). Dokumentasjonen sier **ikke** hvordan
et sluttbrukersystem får den. Vår slutning, ikke Skatteetatens ord:
kontrollsaken er en dialog i Dialogporten som Skatteetaten oppretter, og
id-en hentes derfra (`digdir:dialogporten`, bestilt i #180) eller limes
inn av brukeren fra Altinn-innboksen. Bekreftes før koden skrives.

**Størrelsesgrense: ikke dokumentert** — verken i API-dokumentasjonen
eller i OpenAPI-en. Den eneste grensen Skatteetaten oppgir (15 MB per
fil, 10 filer, ikke zip) gjelder **e-posttestinnsendingen** til
`saft.testinnsending@skatteetaten.no`, ikke API-et.

**Test** krever en kontrollsak på en syntetisk Tenor-organisasjon, som
Skatteetaten oppretter etter henvendelse til brukerstøtte (fanen Test).

**Bestilt 2026-11-27, sak SSV-6691** på desken (eksternjira.sits.no,
kategori Innrapportering, test), sammen med de andre manglende scopene
(docs/gov.md, punkt 3 under «Gjenstår»): `skatteetaten:innleveringkontroll`, en
kontrollsak på en syntetisk Tenor-organisasjon, og spørsmålene om
største filstørrelse, hvordan et sluttbrukersystem får `dialogid`, og
hvilken id `PUT /innsending` skal ha. `ske-kontroll` er lagt til
systemets `rights` i TT02 2026-11-27, og testselskapets systembruker
har fått den (endringsforespørselen godkjent samme dag, docs/gov.md).

Ingen kode skrives før svarene foreligger.
