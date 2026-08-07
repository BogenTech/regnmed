# regnmed documentation

Audit-facing documentation: what the system guarantees, where each
guarantee is enforced, and where it is tested. Written for revisorer,
certification processes, and developers joining the project. Every
milestone updates the relevant document in the same change (policy in
CLAUDE.md).

| Document | Covers |
| --- | --- |
| [api.md](api.md) | Den offentlige API-referansen: alle endepunkter, felles regler, stabilitet |
| [integrations.md](integrations.md) | Maskin-tilgang: client_credentials-identitet, grant per selskap, attribusjon, ratebegrensning |
| [ledger.md](ledger.md) | The append-only, hash-chained ledger: the three immutability layers, verification, and the trust model |
| [anchoring.md](anchoring.md) | External anchoring: Merkle snapshots of chain heads, the public root feed, RFC 3161 witnesses |
| [mva.md](mva.md) | VAT: standard codes, dated rates, beregning rules, mva-spesifikasjon, mva-melding |
| [rapporter.md](rapporter.md) | Lovpålagte spesifikasjoner: bokførings-/kontospesifikasjon, saldobalanse, resultat og balanse |
| [hovedbok.md](hovedbok.md) | Kontoplan (standardkatalog + egne kontoer), drill-down per konto, manuell bilagsføring |
| [saft/README.md](saft/README.md) | SAF-T Financial export and the vendored official artifacts |
| [reskontro.md](reskontro.md) | Kunde-/leverandørspesifikasjon, åpne poster, hash format v2 |
| [dimensjoner.md](dimensjoner.md) | Avdeling/prosjekt på posteringene, hash format v3, resultat per dimensjon |
| [timer.md](timer.md) | Timeføring: heltallsminutter, månedslås, fakturagrunnlag gjennom fakturaflyten |
| [faktura.md](faktura.md) | Utgående faktura: gap-free numbers, KID, kreditnota |
| [produkter.md](produkter.md) | Produktregister (kopiert ved utstedelse) og enkelt varelager: insert-only bevegelser, gjennomsnittskost, varetelling |
| [periodisering.md](periodisering.md) | Periodisering: fordeling av kostnad og inntekt over månedene de hører hjemme i (rskl. §4-1) — aldri av merverdiavgiften |
| [balansedokumentasjon.md](balansedokumentasjon.md) | Balansedokumentasjon: hva hver balansepost består av ved periodeslutt (bokføringsloven §11) — manglende dokumentasjon er et avvik |
| [kontantsalg.md](kontantsalg.md) | Kontantsalg: kontantfaktura (§5-3) og kassaoppgjør fra et kassasystem (§5-4) — kassadifferansen alltid som eget bilag |
| [anlegg.md](anlegg.md) | Anleggsregister: lineære avskrivninger som ordinære bilag, skattemessig saldo per gruppe, avhending m/ gevinst/tap |
| [utlegg.md](utlegg.md) | Utlegg og kjøregodtgjørelse: uforanderlige krav, enveis beslutninger, statens satser fra satsregisteret |
| [valuta.md](valuta.md) | Flervaluta: hash format v4, daterte kurser fra Norges Bank, realisert agio i samme transaksjon som matchen |
| [purring.md](purring.md) | Betalingsoppfølging: aldersfordeling, purregebyr/forsinkelsesrente som bilag, inkassovarsel |
| [budsjett.md](budsjett.md) | Budsjett og avviksrapport: arbeidsdokument til det fastsettes, versjoner en rapport kan navngi |
| [perioder.md](perioder.md) | Periodelåsing (ajourhold) and bilagsvedlegg (oppbevaringsplikt) |
| [bilagsinnboks.md](bilagsinnboks.md) | The client→accountant inbox: immutable uploads, atomic bokføring |
| [epost-inn.md](epost-inn.md) | E-post-inn til innboksen: adressen som kapabilitet, ukjent avsender i karantene, én mail-rail |
| [bilagstolkning.md](bilagstolkning.md) | Forslag fra dokumentets egen tekst: PDF-tekstlag, kontrollsiffer-heuristikk, kontoforslag fra historikken — aldri automatisk bokføring |
| [attestering.md](attestering.md) | Godkjenningsflyt før bokføring og betaling: valgfri policy, insert-only beslutningsspor, fire øyne håndhevet i transaksjonen |
| [portal.md](portal.md) | The web portal: SPA architecture, OIDC+PKCE, theme contract |
| [marketplace.md](marketplace.md) | Onboarding from BRREG; firm autorisasjon via Finanstilsynet |
| [lonn.md](lonn.md) | Lønn, første del: fastlønn, prosenttrekk, aga per sone og feriepengeavsetning som ett bilag — og den lange listen over hva som ennå IKKE er bygget, a-meldingen først |
| [aksjonaer.md](aksjonaer.md) | Aksjeeierbok (aksjeloven §4-5) og aksjonærregisteroppgaven RF-1086: eierandelen beregnet fra hendelser, fødselsdato i boken og fødselsnummer bare i innsendingen, og kodene vi ikke gjetter |
| [skattemelding.md](skattemelding.md) | Skattemelding og næringsspesifikasjon (#11) — kartlagt, ikke bygget: innsending krever ID-porten, ikke Maskinporten, og versjon-per-inntektsår er ikke publisert |
| [revisjon.md](revisjon.md) | Revisorrollen: read-only access and the one-click verification report |
| [migration.md](migration.md) | SAF-T import: the universal migration path |
| [ehf.md](ehf.md) | EHF/PEPPOL ut og inn: dokumentet vi sender og leser, transporten som egen tier |
| [bank.md](bank.md) | Bank reconciliation: camt.053 import, matching, connectivity tiers |
| [betaling.md](betaling.md) | Betalingsliste og remittering: pain.001, enveis kjøringer, oppgjør som lukker reskontroen |
| [secrets.md](secrets.md) | Ingenting hemmelig i repoet, heller ikke kryptert — hvor Maskinporten-nøkkelen faktisk ligger, og én nøkkel per maskin |
| [auth.md](auth.md) | **Hvem kan gjøre hva, og hvor er det håndhevet** — aktørene, rettighetsvokabularet, de innebygde og egendefinerte rollene, og den maskinsjekkede tilgangsmatrisen |
| [abonnement.md](abonnement.md) | Regnmeds egen kasse: statuser beregnet aldri lagret, prislisten som data, fakturering gjennom egen motor — og sperren som aldri tar hovedboken som gissel |
| [gov.md](gov.md) | The government rail: Maskinporten, Skatteetaten APIs, operational setup |
| [frugality.md](frugality.md) | The resource budget and the CI gate that enforces it |
| [deploy.md](deploy.md) | Base + overlays, production checklist, bootstrap of the first login, verified backups, TLS |
| [regelverk.md](regelverk.md) | Rules as data: dated satser, per-year authority artifacts, the yearly regelverksrevisjon |

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
