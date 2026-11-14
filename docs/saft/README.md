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
