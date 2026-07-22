# SAF-T Financial — official Skatteetaten artifacts

Vendored from [Skatteetaten/saf-t](https://github.com/Skatteetaten/saf-t)
(fetched 2026-07-22) so builds and CI never depend on GitHub being up:

- `Norwegian_SAF-T_Financial_Schema_v_1.30.xsd` — the official schema our
  export is validated against (unit test `validates_against_official_xsd`
  and every real export via `xmllint`).
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

The Norwegian SAF-T header requires a contact person, which is not master
data regnmed holds — hence the mandatory `--contact`. Accounts that are
not themselves standard accounts are mapped to the nearest standard
account and reported on stderr for an accountant's review.
