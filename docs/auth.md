# Identity and authorization

Two deliberately separated concerns:

- **Identity** (who you are): proven by an OIDC token from the IdP.
- **Authorization** (what you may do, for which company): decided by
  regnmed's own database. Never carried in tokens — an accountant with 60
  clients cannot meaningfully carry that in a JWT, and access changes
  must take effect without re-login.

## Identity: OIDC relying party only

The IdP is **regnid** (sibling repo, our Rust port of networco-id).
regnmed validates RS256 tokens against a configured issuer/JWKS
(`crates/regnmed-api/src/auth.rs`: `Verifier` + the `AuthPerson`
extractor — adding `AuthPerson` as a handler argument protects the
route). regnmed never bakes in IdP specifics; any spec-compliant issuer
works. Config: `OIDC_ISSUER`, optional `OIDC_AUDIENCE`,
`OIDC_JWKS_FILE` (dev/tests: static JWKS, signatures still validated).

Rejected with 401, verified by tests: missing/garbage tokens, tokens
signed by the wrong key, expired tokens, wrong audience.

## Authorization: the engagement model

Migration 0005 (`person`, `firm`, `firm_member`, `company_member`,
`engagement`):

```
person ──── company_member ────────────────► company   ("direkte")
   │
   └─────── firm_member ──► firm ── engagement (oppdrag) ──► company
```

- An **engagement** (oppdrag) is the first-class relationship between a
  regnskapsfører-/revisorfirma and a client company, with scope
  (regnskap/revisjon) and validity. Revisor engagements are read-only +
  chain verification.
- `/me` resolves token → person (JIT-provisioned on first login) → all
  companies the person may act for, each with its access level and the
  path it came through (`via` = firm name or "direkte").
- This mirrors Altinn's delegation model (see gov.md), which will let
  government-side delegation and regnmed-side engagements stay aligned.

## Per-company guard on API routes

Every company-scoped endpoint goes through **one** guard,
`regnmed_api::tilgang::krev` — the only place in the API that calls
`regnmed_db::company_access`. No path to the company yields **404, not
403**: a caller without access must not learn that the company exists.

Endepunktet sier hvilken **rettighet** handlingen krever, ikke hvem som
får gjøre den. Rettigheten hører til handlingen og endrer seg ikke når
vi legger til en rolle:

```rust
krev(&state, person.person_id, company_id, Rett::FakturaSkriv).await?;
```

`krev` returnerer rollen, så en handler som trenger mer enn ja/nei
slipper et nytt oppslag — periodelåsen bruker det: å låse krever
`PERIODE_LAAS`, å **åpne igjen** krever admin.

## Rettigheter og roller

**En rolle er et sett rettigheter, ikke et trinn på en stige.** Fram
til #59 var tilgang tre nivåer (`admin` > `bokforing` > `les`), lett å
forstå og umulig å bøye: enten så du hele hovedboken, eller ingenting.
Et selskap som vil ha «en som bare fakturerer» eller «en controller som
ser alt bortsett fra lønn» hadde ingen vei.

Vokabularet er en **enum i koden** (`regnmed_api::tilgang::Rett`), ikke
fritekst i databasen: et endepunkt kan ikke kreve en rettighet som ikke
finnes, og kompilatoren finner alle stedene når en rettighet endrer
navn. Navnene er norske, som resten av domenet — `FAKTURA_LES`, ikke
`INVOICE_READ`.

**Rettigheter er additive, aldri subtraktive.** Det finnes ingen «alt
unntatt X»; den regelen er umulig å resonnere om når roller settes
sammen.

De tre innebygde rollene er faste bunter:

| Bunt | Innhold |
| --- | --- |
| `les` | `*_LES` for bilag, rapporter, faktura, reskontro, bank, betaling, produkter, lager, anlegg, budsjett, dimensjoner, aksjebok, utlegg, lønn, forankring, oppdrag, integrasjoner |
| `bokforing` | `les` + alt som endrer hovedboken: `BILAG_BOKFOR`, `FAKTURA_SKRIV`, `BANK_AVSTEM`, `BETALING_OPPRETT`/`_GODKJENN`/`_OPPGJOR`, `LONN_KJOR`, `PERIODE_LAAS` … |
| `admin` | `bokforing` + `SELSKAP_ADMIN`, `MEDLEM_ADMIN`-slekten, `INTEGRASJON_ADMIN`, `OPPDRAG_ADMIN`, `TIMER_LAAS`, `TIMER_*_ALLE`, `MIGRERING_ADMIN`, `MVA_ORDNING_ADMIN` |

At de er nøstet er en egenskap ved **disse tre**, ikke ved modellen. En
egendefinert rolle (#60) trenger ikke være nøstet i det hele tatt.

### Omfang: egne data mot alles

Noen rettigheter finnes i par, `_EGNE`/`_ALLE`. En ansatt skal føre sine
egne timer uten å se kollegenes; en leder skal se begge deler.

- **`_ALLE` medfører `_EGNE`.** Ellers måtte hver bunt huske begge, og
  en bunt som glemte `_EGNE` ville stengt folk ute fra deres egne data.
- Et endepunkt som allerede filtrerer på personen krever `_EGNE`; et
  som viser eller endrer andres krever `_ALLE`.

Dimensjonen ble bestemt i #59, ikke utsatt, nettopp fordi den er dyr i
ettertid: hver «egen»-variant ville blitt et nytt navn, og lagrede
roller måtte migreres. Timeføringen bruker den allerede — listen er
egne timer, admin retter alles.

**Kjent svakhet:** `LONNSSLIPP_LES` ligger i `les`-bunten, så enhver med
tilgang kan laste ned andres lønnsslipp. Det er dagens oppførsel, ikke
en ny beslutning — #59 skulle ikke endre atferd. #55 retter det.

**Hvorfor én vakt.** Fram til #56 hadde hver modul sin egen
`require_access` — 22 kopier i tre ulike former (`write: bool`,
`admin: bool`, et nivå som streng, og noen uten parameter i det hele
tatt). Så lenge regelen var «les eller skriv» gikk det bra, men hver
kopi var et sted å ta feil, og en fjerde rolle ville måttet skrives inn
22 ganger. Samme begrunnelse som ratebegrensningen for integrasjoner:
én søm ingen kan glemme.

En ukjent rolleverdi faller til **svakeste** rolle, ikke til en feil. En
datafeil skal ikke kunne bli en tilgangseskalering.

## Where it is tested

- `crates/regnmed-api/tests/me_endpoint.rs` (real Postgres, also CI): a
  locally generated JWKS signs real RS256 tokens; a seeded
  firm-with-engagement plus a direct membership resolve to exactly the
  expected company list; the forged/expired/wrong-audience matrix is
  rejected.
- `regnmed_api::tilgang` sine enhetstester — at buntene ikke
  overlapper, at slug-ene er unike, at `_ALLE` medfører `_EGNE`, og at
  en ukjent rolleverdi faller til svakeste rolle. **De kan ikke fange
  at en rettighet ligger i feil bunt** — de utleder fasiten fra
  buntene. Prøvd med vilje: flyttet `PRODUKT_SKRIV` til lesebunten, og
  alle åtte besto. Det er matrisetesten under som er sperren der.
- `crates/regnmed-api/tests/tilgang.rs` — tilgangsmatrisen, skrevet som
  **nektelser**: at `les` ikke får endre noe, at `bokforing` ikke får
  administrere, og at en utenforstående får 404 og ikke 403 på hvert
  eneste endepunkt i utvalget. At en admin slipper til er dekket
  overalt ellers; at en leser ikke slipper til er det ingenting annet
  som fanger. Testen ble kontrollert ved å ødelegge én vakt med vilje —
  den slo ut.

  Merk at kroppene i testen må være **gyldige JSON for endepunktet**:
  axum kjører `Json<T>`-uttrekket før handleren, så en tom kropp gir
  422 og vakten blir aldri spurt. En matrise med `{}` ville bestått
  uten å bevise noe.
