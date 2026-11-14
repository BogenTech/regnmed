# SAF-T Financial — official Skatteetaten artifacts

Vendored from [Skatteetaten/saf-t](https://github.com/Skatteetaten/saf-t)
(first fetched 2026-07-22; both schemas refreshed 2026-11-14 at commit
`b54f1cf`) so builds and CI never depend on GitHub being up:

- `Norwegian_SAF-T_Financial_Schema_v_1.30.xsd` (SHA-256
  `5362d677…df6a0650`) and `Norwegian_SAF-T_Financial_Schema_v_1.40.xsd`
  (SHA-256 `272d5a3e…76b89d01`) — the official schemas our export is
  validated against (unit test `validates_against_official_xsd`, both
  versions, and every real export via `xmllint`).
- `About_SAF-T_Financial_schema_v.1.40.txt` and
  `Bug fix in version 1.30 and version 1.40.txt` — Skatteetaten's notes on
  what 1.40 changed and on the 2026-10-16 correction that made
  `SelectionCriteria` mandatory in both schemas (no version bump). Our
  1.30 copy was from before that fix; the refresh picked it up, and the
  export has always written `SelectionCriteria`.
- `generated_auditfile_formatted_v_1.40.xml` — Skatteetaten's 1.40
  example file; the importer test `imports_skatteetatens_1_40_example`
  reads it.
- `naeringsspesifikasjon_2025-2026.csv` — grouping category code list
  (standard account → GroupingCategory/GroupingCode). A copy is embedded
  in `regnmed-core` (`src/saft/`); when Skatteetaten publishes the list
  for a new inntektsår, update both copies together.
- `Standard_Tax_Codes.csv` — the standard VAT code list; regnmed's
  `vat_code` table uses these codes directly, which is why the export can
  set `StandardTaxCode` = our own code.
- `About_SAF-T_Financial_v.1.3.txt` — Skatteetaten's release notes.

Export a file with:

```sh
regnmed saft-export --orgnr 999888777 --year 2026 --contact "Kari Nordmann"
```

## Which version a file is written in

`regnmed-core::saft::schema_version` picks it from the selection's start
date: **1.40 for a period starting 1 January 2027 or later, 1.30 before
that**. 1.40 comes into force for the first period starting on or after
1 January 2027 and is voluntary until then; 1.30 stays valid for 2026 and
earlier. A selection that starts in 2026 and runs into 2027 is therefore
written as 1.30, the same way the start date already picks the
grouping-code vintage.

Every 1.40 change is backward compatible, and none needs new data from us:

- `Owners/AccountID` may repeat: we do not write `Owners`.
- `CurrencyAmount` and `ExchangeRate` are wider decimals: our values fit
  both versions.
- `VirtualCurrencyType` and `Country` in the amount structure are
  optional: we do not use them.
- `DebitNOKTaxAmount`/`CreditNOKTaxAmount` are only for a `TaxInformation`
  amount in a currency other than NOK: ours are always NOK (the VAT is
  posted in NOK, bokføringsloven § 4-2).

So the same document validates against both schemas, and the version
string is the only difference. **A 2027 export still fails today**, and
deliberately: Skatteetaten has not published the næringsspesifikasjon
grouping list for 2027 yet, and the export refuses a year no vendored list
covers (docs/regelverk.md). Vendoring that list is a December
regelverksrevisjon item.

The importer reads both versions: it reads elements by name, and the 1.40
additions are elements it does not read.

The Norwegian SAF-T header requires a contact person, which is not master
data regnmed holds — hence the mandatory `--contact`. Accounts that are
not themselves standard accounts are mapped to the nearest standard
account and reported on stderr for an accountant's review.
