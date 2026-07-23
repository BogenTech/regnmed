# regnmed documentation

Audit-facing documentation: what the system guarantees, where each
guarantee is enforced, and where it is tested. Written for revisorer,
certification processes, and developers joining the project. Every
milestone updates the relevant document in the same change (policy in
CLAUDE.md).

| Document | Covers |
| --- | --- |
| [ledger.md](ledger.md) | The append-only, hash-chained ledger: the three immutability layers, verification, and the trust model |
| [anchoring.md](anchoring.md) | External anchoring: Merkle snapshots of chain heads, the public root feed, RFC 3161 witnesses |
| [mva.md](mva.md) | VAT: standard codes, dated rates, beregning rules, mva-spesifikasjon, mva-melding |
| [rapporter.md](rapporter.md) | Lovpålagte spesifikasjoner: bokførings-/kontospesifikasjon, saldobalanse, resultat og balanse |
| [saft/README.md](saft/README.md) | SAF-T Financial export and the vendored official artifacts |
| [reskontro.md](reskontro.md) | Kunde-/leverandørspesifikasjon, åpne poster, hash format v2 |
| [faktura.md](faktura.md) | Utgående faktura: gap-free numbers, KID, kreditnota |
| [perioder.md](perioder.md) | Periodelåsing (ajourhold) and bilagsvedlegg (oppbevaringsplikt) |
| [bilagsinnboks.md](bilagsinnboks.md) | The client→accountant inbox: immutable uploads, atomic bokføring |
| [portal.md](portal.md) | The web portal: SPA architecture, OIDC+PKCE, theme contract |
| [marketplace.md](marketplace.md) | Onboarding from BRREG; firm autorisasjon via Finanstilsynet |
| [revisjon.md](revisjon.md) | Revisorrollen: read-only access and the one-click verification report |
| [migration.md](migration.md) | SAF-T import: the universal migration path |
| [bank.md](bank.md) | Bank reconciliation: camt.053 import, matching, connectivity tiers |
| [auth.md](auth.md) | Identity (OIDC) and authorization (engagement model) |
| [gov.md](gov.md) | The government rail: Maskinporten, Skatteetaten APIs, operational setup |
| [frugality.md](frugality.md) | The resource budget and the CI gate that enforces it |

Conventions used everywhere:

- **Money is integer øre** (`Ore(i64)`), positive = debit, negative =
  credit. Floats never touch monetary or tax arithmetic.
- **Balances are queries** — always `SUM(amount_ore)`, never stored
  mutable state.
- **Norwegian domain terms** (bilag, termin, grunnlag, oppdrag) are kept
  untranslated in code, reports and documents.
- Formats owned by authorities (SAF-T, mva-melding) are validated against
  the authority's own published XSD, vendored in this repo, on every test
  run and in CI.
