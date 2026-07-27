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

## Medlemsadministrasjon (#53)

Fram til migrasjon 0037 kunne **ingen gi noen tilgang**. De to veiene inn
var å opprette selskapet selv (og bli admin) eller å få et oppdrag fra
et byrå — et selskap kunne altså ikke ta inn sin egen interne
regnskapsfører eller en ansatt.

Nå: `MEDLEM_ADMIN` gir rett til å invitere, endre rolle og fjerne
tilgang, under `/companies/{id}/access…` og `/companies/{id}/invitations…`.

### Invitasjonen er stilet til en adresse, ikke til en person

`person` opprettes just-in-time fra tokenets `sub`, så en som aldri har
brukt regnmed **finnes ikke å slå opp**. En invitasjon peker derfor på en
e-postadresse og blir til et medlemskap når adressen logger inn. Det
løses inn i `/me`, altså når portalen starter en økt — samme mønster som
oppdrag, der tilgangen blir synlig uten ny innlogging. Å legge oppslaget
i `AuthPerson`-ekstraktoren ville kostet en spørring på *hver*
forespørsel for noe som skjer én gang.

**Adressen må være den IdP-en oppgir.** `ensure_person` skriver
`person.email` fra hver innlogging, så en invitasjon til en privat
adresse blir aldri løst inn om tokenet bærer jobbadressen.
Normaliseringen (trimming og små bokstaver) skjer ett sted,
`medlemmer::normaliser_epost`, og det er den normaliserte formen som
lagres.

**Svaret røper ikke om adressen alt har en bruker hos oss.** Et oppslag
«finnes denne e-posten» ville gjort enhver selskapsadmin i stand til å
kartlegge hvem som er bruker på plattformen, ett forsøk om gangen. Både
det kjente og det ukjente tilfellet gir samme svar, og det har sin egen
test.

### Det som ikke kan skje

- **Et selskap kan ikke bli stående uten administrator.** Den siste kan
  verken degradere eller fjerne seg selv. Kontrollen kjører inne i
  transaksjonen, etter endringen, med `for update` på selskapets
  medlemsrader først — uten låsen kunne to samtidige degraderinger begge
  se «det finnes en annen admin».
- **Tilgang gjennom et oppdrag kan ikke endres her.** Den følger
  engasjementet. Listen viser vedkommende, merket `kan_endres: false`, og
  et forsøk avvises med en forklaring i stedet for å se ut som om det
  virket.
- **Medlemskap slettes aldri**, det deaktiveres — det er historikken over
  hvem som hadde tilgang.

Hver endring havner i `company_member_change`, som er innsettings-bar:
hvem fikk hva, når, og hvem som ga det. En innløst invitasjon har ingen
utfører (personen løste den inn selv); hvem som inviterte står på
invitasjonen.

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

`krev` returnerer personens samlede tilgang, så en handler som trenger
mer enn ja/nei slipper et nytt oppslag — periodelåsen bruker det: å låse
krever `PERIODE_LAAS`, å **åpne igjen** krever admin.

**Rettighetene unioneres over alle veier inn.** En person kan være
direkte medlem *og* komme inn via et oppdrag. Så lenge rollene var en
stige holdt det å velge den sterkeste; etter `ansatt` (#54) er de ikke
det, og en ansatt som også kom inn gjennom et byrå ville mistet retten
til å føre sine egne timer hvis vi valgte én. `company_access` finnes
fortsatt, men bare til visning — tilgang avgjøres av `company_roles` og
unionen.

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

De innebygde rollene er faste bunter:

| Bunt | Innhold |
| --- | --- |
| `ansatt` | **Selvbetjening** (#54): `TIMER_LES_EGNE`, `TIMER_SKRIV_EGNE`, `UTLEGG_LES_EGNE`, `UTLEGG_SKRIV_EGNE`, `LONNSSLIPP_LES_EGEN`, `BILAG_LAST_OPP`, `DIMENSJON_LES` |
| `les` | `*_LES` for bilag, rapporter, faktura, reskontro, bank, betaling, produkter, lager, anlegg, budsjett, dimensjoner, aksjebok, utlegg, forankring, oppdrag, integrasjoner — **ikke lønn** |
| `revisor` | `les` + `LONN_LES` og `LONNSSLIPP_LES_ALLE` (#55). Kommer bare fra et revisjonsoppdrag; kan ikke tildeles direkte |
| `bokforing` | `les` + alt som endrer hovedboken: `BILAG_BOKFOR`, `FAKTURA_SKRIV`, `BANK_AVSTEM`, `BETALING_OPPRETT`/`_GODKJENN`/`_OPPGJOR`, `LONN_KJOR`, `PERIODE_LAAS` … + lønnslesingen |
| `admin` | `bokforing` + `SELSKAP_ADMIN`, `MEDLEM_ADMIN`, `INTEGRASJON_ADMIN`, `OPPDRAG_ADMIN`, `TIMER_LAAS`, `TIMER_*_ALLE`, `MIGRERING_ADMIN`, `MVA_ORDNING_ADMIN` |

`les` ⊂ `revisor` ⊂ `bokforing` ⊂ `admin` er en egenskap ved **disse
fire**, ikke ved modellen. En egendefinert rolle er ikke nøstet i det
hele tatt — og `ansatt` er det ikke.

### Egendefinerte roller (#60)

Et selskap setter sammen sine egne roller av rettighetene som finnes:
«Fakturaansvarlig», «Controller uten lønn», «Lagermedarbeider».
`company_role` + `company_role_right`, per selskap, og
`company_member.role` peker på navnet.

**De innebygde rollene ligger IKKE i databasen.** De er definert i
`regnmed-api::tilgang` og blir der. To definisjoner av det samme er to
steder å drive fra hverandre, og et regnskapssystem har ikke råd til at
tilgang betyr én ting i koden og en annen i basen. Tabellen holder bare
det selskapet har funnet på selv; portalen får de innebygde beskrevet
fra koden via `GET …/roles`.

**Rollenavnet er permanent**, som en dimensjonskode og av samme grunn:
det er nøkkelen medlemskapene peker på. Kunne det endres, ville de pekt
på en rolle som ikke finnes lenger. Vil man ha et annet navn, lager man
en ny rolle. De innebygde navnene er reservert, ellers ville en
selskapsdefinert «admin» skygget for den ekte.

**Rettigheter som styrer hvem som har tilgang kan ikke delegeres.**
`MEDLEM_ADMIN`, `SELSKAP_ADMIN`, `OPPDRAG_ADMIN` og
`INTEGRASJON_ADMIN` er utenfor rekkevidde for en egendefinert rolle
([`Rett::kan_delegeres`]) — en rolle som kan endre tilganger kan gi seg
selv alt annet, og da er resten av avgrensningen bare pynt. De blir
værende hos `admin`, som er en rolle et selskap ikke kan skrive om.
Avvist når rollen lages, ikke bare ignorert ved oppslag.

**Ukjente rettighetsnavn oppfører seg forskjellig i de to retningene**,
og det er med vilje: når et *menneske* skriver en rolle, avvises et
ukjent navn høylytt (en rolle som stilltiende mangler halve innholdet er
verre enn en feilmelding); ved *oppslag* ignoreres det (der er det en
gammel database eller en tilbakerullet versjon, ikke en skrivefeil).
Databasen kjenner ikke vokabularet, så en rolle kan aldri love en
rettighet ingen håndhever.

En rolle **slettes aldri**, den deaktiveres — og en deaktivert rolle gir
ingenting. Medlemskapet står, så historikken om hvem som hadde hvilken
tilgang er intakt. Endringene ligger i `company_role_change`.

Migrasjon 0039 fjernet check-constraint-en på `company_member.role`,
siden listen ikke lenger er kjent for databasen. Det svekker ikke
vernet: oppslaget er **fail-closed** — et rollenavn koden ikke kjenner
igjen gir ingen rettigheter, så en ugyldig verdi stenger ute i stedet
for å slippe inn.

Integrasjoner (#45) går gjennom samme oppslag, så en maskin kan få en
egendefinert rolle og dermed nøyaktig `FAKTURA_LES` og ingenting mer.
At `admin` ikke er grantbart til en maskin står fortsatt.

### Lønn er ikke allmenn lesning

Fram til #55 lå `LONNSSLIPP_LES_ALLE` og `LONN_LES` i lesebunten, så
**enhver med lesetilgang kunne laste ned hvem som helst sin
lønnsslipp** — bruttolønn, forskuddstrekk, feriepenger og fødselsdato —
og se ansattlisten med månedslønn og trekkprosent. Det var ikke en
beslutning; lønn kom sist og gjenbrukte den generelle lesetilgangen.

Rettingen tvang fram et skille som uansett var riktig: **`revisor` og en
intern leser er ikke det samme.** Begge er skrivebeskyttet, men bare den
ene har en revisjonsplikt som krever lønnsopplysningene — lønn er en
vesentlig kostnad, og forskuddstrekk og arbeidsgiveravgift er lovpålagte
størrelser som skal kunne kontrolleres. Før #55 var de samme streng:
et `revisjon`-oppdrag ga `les`. Nå gir det `revisor`.

Svaret på «hvor mye ser en revisor?» er altså fortsatt «lønn også», men
det er nå et **uttrykkelig ja** i stedet for en bieffekt. En intern
leser — et styremedlem, en controller — ser det ikke lenger.

**Ingen egen `lonn`-rolle.** Vurdert og valgt bort: den som fører
regnskapet må uansett se lønn for å bokføre den, så en rolle som skiller
dem ville bare hatt mening sammen med egendefinerte roller (#60). Der
kan et selskap lage «controller uten lønn» selv, av rettighetene som nå
finnes.

### Ansattrollen er ikke «lesing minus noe»

Den er den formen rangstigen ikke kunne uttrykke: en ansatt får
**skrive** noen få ting — sine egne timer, sitt eget refusjonskrav, et
bilde av en kvittering — og **lese nesten ingenting**. Før #54 måtte man
gi bort bokføringstilgang til hele hovedboken for å la noen føre timer.

Bunten er positivt avgrenset: den lister hva en ansatt får, ikke hva hun
er nektet. Hovedbok, rapporter, faktura, bank, reskontro, ansattlisten
og alles timer, utlegg og lønn står utenfor, og havner ikke innenfor ved
et uhell — en ny rettighet må skrives inn i bunten for å gjelde.

To ting kom ut av å bygge den:

- **`TIMER_RAPPORT_LES`** måtte skilles fra `TIMER_LES_ALLE`. De
  selskapsvide timeoversiktene (per prosjekt, ufakturert) er ikke den
  ansattes sak, men de er heller ikke det samme som admins rett til å se
  og rette andres enkelttimer. Uten skillet ville enten en leser mistet
  totalene, eller en ansatt fått dem.
- **Ukjent rolleverdi gir nå ingen rettigheter**, ikke `les`. Før #54 var
  `les` svakest, så «ukjent → les» var trygt. Nå er `ansatt` og `les`
  ikke sammenlignbare, og under en rullerende utrulling kan en gammel
  binær møte `ansatt` i databasen — å tolke den som `les` ville vært en
  **oppgradering**, ikke en degradering.

Portalen viser en ansatt bare Timer, Utlegg og en liten Bilag-side med
kvitteringsfoto. Det er en bekvemmelighet, ikke en sperre: **portalen
skjuler, serveren nekter.**

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

Etter #54 gjelder det disse parene: `TIMER_LES_*`, `TIMER_SKRIV_*`,
`UTLEGG_LES_*` og `LONNSSLIPP_LES_*`. Omfanget håndheves i handleren,
ikke i spørringen alene: lønnsslippen og kvitteringen til en annen
svarer **404**, ikke 403 — den som ikke får se noe skal heller ikke få
vite at det finnes.

**Kjent svakhet:** `LONNSSLIPP_LES_ALLE` ligger fortsatt i `les`-bunten,
så enhver med lesetilgang kan laste ned andres lønnsslipp. Det er dagens
oppførsel, ikke en ny beslutning — verken #59 eller #54 skulle endre
hvem som har hva. #55 retter det.

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
- `crates/regnmed-api/tests/medlemmer.rs` — hele livsløpet: invitasjon →
  innlogging → medlemskap, at svaret ikke røper om brukeren finnes, at
  siste admin ikke kan fjerne seg selv, at oppdragstilgang ikke kan
  endres herfra, at en bokfører ikke kan slippe noen inn, og at sporet
  navngir hvem som ga hvem tilgang.
- Ansattrollen har egne rader i matrisen: en bred liste over det hun
  IKKE når (hovedbok, rapporter, faktura, bank, betaling, ansattlisten,
  innboksen, selskapsvide timeoversikter), det hun SKAL nå, at hun kan
  laste opp en kvittering uten å kunne lese innboksen, og at hun får sin
  egen lønnsslipp men får 404 på kollegaens.
- `crates/regnmed-api/tests/tilgang.rs` — tilgangsmatrisen, skrevet som
  **nektelser**: at `les` ikke får endre noe, at `bokforing` ikke får
  administrere, og at en utenforstående får 404 og ikke 403 på hvert
  eneste endepunkt i utvalget. At en admin slipper til er dekket
  overalt ellers; at en leser ikke slipper til er det ingenting annet
  som fanger. Testen ble kontrollert ved å ødelegge én vakt med vilje —
  den slo ut.

  Merk at både kroppene og **spørrestrengene** må være gyldige for
  endepunktet: axum kjører `Json<T>`- og `Query<T>`-uttrekkene før
  handleren, så en tom kropp gir 422 og en manglende parameter gir 400 —
  og vakten blir aldri spurt. En matrise med `{}` ville bestått uten å
  bevise noe. Begge fellene er gått i under bygging, og begge ga en
  test som så grønn ut mens den målte ingenting.
