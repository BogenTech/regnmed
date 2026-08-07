# Marketplace: onboarding from the official registries

The marketplace's trust starts at onboarding: **names and facts come
from the registries, never from user input**, and firms may only offer
services after their autorisasjon is verified.

## The registration flow (#77)

Two fundamentally different paths lead into the system, and the portal
routes the first-time user **before** asking for anything (badge cards
on the Companies view; existing users reach the same flow via
"Registrer nytt selskap eller byrå"):

1. **Egen virksomhet** → company onboarding below. 30-day prøvetid,
   then abonnement (docs/abonnement.md).
2. **Regnskapsfører eller revisor** → firm verification below. **Free
   for the byrå** — all billing is per company; firms have no billing
   tables at all. Clients each carry their own prøvetid/abonnement.
3. **Jeg er invitert** → nothing to create: invitations are redeemed
   automatically by `/me` at login (docs/auth.md §7). The portal shows
   which address the invitation must be sent to, offers a re-check, and
   surfaces `nye_tilganger` when the redemption happens.

Both creating paths share the registry preview, and its autorisasjon
flags steer the routing **both ways as suggestions, never decisions**:
an orgnr with active autorisasjon looked up under "egen virksomhet"
gets a "register as byrå instead?" hint (a byrå may legitimately also
keep its own books as a company), and an orgnr without autorisasjon
under the byrå path is told why firm registration is impossible and
offered the company path. Slettet/konkurs blocks both paths — also
enforced server-side on both endpoints, not just for companies.

Connecting further users to a created company (lønnsmottakere,
økonomiansvarlig, …) is #79; the byrå's own employees are #78.

## Company onboarding (Enhetsregisteret)

`POST /companies {orgnr}` (portal: "Nytt selskap fra Enhetsregisteret"):

1. Orgnr is checksum-validated (MOD11, `regnmed-core::orgnr`) before any
   lookup.
2. Facts are fetched from BRREG's open API (`regnmed-gov::brreg`;
   `BRREG_API_URL` overrides for tests/mirrors). Enheter that are
   **slettet, konkurs, under avvikling or under tvangsavvikling** are
   refused — the last two are not bankruptcy, so the konkurs check
   alone missed them.
3. The company is created with the **registry name**, the onboarding
   person becomes admin, and a starter NS 4102 kontoplan (10 accounts)
   is seeded with 1500/2400 flagged as kunde-/leverandør-reskontro —
   invoice-ready from the first minute. An orgnr can only be onboarded
   once.

### What else is taken from the register

The principle is that regnmed should not ask the user to type what
Enhetsregisteret already knows, and should not *derive* what it can
*read*:

| Felt | Brukes til |
| --- | --- |
| `forretningsadresse` → `company.address` | påkrevd på salgsdokumentet (§5-1-2) — utstedelse nekter uten |
| `organisasjonsform` → `company.orgform` | firmaopplysninger |
| `epostadresse` → `company.email` | svaradresse på utsendelser |
| `registrertIMvaregisteret` / `-IForetaksregisteret` **med registreringsdato** | påtegningene «MVA»/«Foretaksregisteret», som daterte rader |
| `naeringskode1` → `company.naeringskode` | næringsspesifikasjonen (#11); ingen konsument ennå |
| `kapital` → `aksjekapital_ore`, `antall_aksjer` | kontrolltall for aksjeeierboken (#43); bokføres aldri |

**Registreringsdatoene er poenget, ikke flaggene.** De to registrene har
uavhengige datoer — Equinor kom i Foretaksregisteret i 1988 og i
Merverdiavgiftsregisteret i 1989 — så onboarding skriver ÉN RAD PER
ENDRING (`BrregEnhet::registreringstidslinje`), ikke én rad datert i dag.
Et dokument datert mellom dem bærer da nøyaktig de påtegningene som
gjaldt. Uten dette ville et selskap som registrerte seg for mva i 2019
og onboardes nå, mangle «MVA» på all importert historikk.

Aksjekapitalen kommer som et JSON-**tall** og parses desimalt til øre
(samme parser som valutakursene) — ingen float rører penger. Kapital i
annen valuta enn NOK lagres ikke i stedet for å regnes om.

`GET /registry/enheter/{orgnr}` previews the facts (incl. autorisasjon
flags) before anything is created.

## Firm verification (Finanstilsynets register)

`POST /firms {orgnr, kind}` (portal: the byrå path of the registration
flow) creates a regnskapsfører-/revisorfirma **only** when the orgnr
holds an active autorisasjon of that kind:

- Slettede and konkurs-registrerte enheter are refused before the
  autorisasjon gate — same registry-facts rule as companies.
- The check **fails closed**: unconfigured or unreachable register, or
  unknown orgnr, all mean "not verified" — nobody becomes a firm because
  a lookup happened to break.
- The verification moment and source are recorded on the firm
  (`autorisasjon_verified_at`, `autorisasjon_ref`) — audit trail for the
  directory (#23) and for revisjon.

**Adapter status**: Finanstilsynets virksomhetsregister is public, but
its API endpoint is not stably documented, so
`regnmed-gov::finanstilsynet` is a thin adapter behind
`FINANSTILSYNET_API_URL` expecting
`GET {base}/virksomheter/{orgnr}` → `{"autorisasjoner":[{"kode","aktiv"}]}`.
The URL and field mapping get pinned against the live register during
pilot onboarding; the enforcement point, flow and tests are real today
and only the adapter may move. Re-verification cadence (licenses can be revoked) is a pilot decision.

## Firm membership (byråmedlemmer, #78)

A firm member reaches **every client of the firm** through the
engagements (the access model resolves `firm_member → engagement →
company` live), so letting someone into the byrå is letting them into
its client portfolio. That shapes every rule here:

- **First come, first served — then the gate closes.** Registration
  (`POST /firms`) refuses an orgnr that is already a firm; it used to be
  idempotent, which silently made the second registrant a co-admin of
  the existing byrå. Everyone after the founder enters by invitation
  from a firm admin. (Companies have always worked this way: an orgnr
  onboards once.)
- **Invitations mirror the company discipline** (migration 0046 ≈ 0037):
  addressed to an e-mail address, redeemed by `/me` at login, the
  response never reveals whether the address already has a user, one
  open invitation per firm/address, the mail (same rail, same
  `utsendelse` log) carries no secret token. An existing membership is
  never silently upgraded by a redemption.
- **Roles**: `admin` runs the byrå (members, invitations, engagement
  decisions), `ansatt` works in it (sees requests and clients, decides
  nothing). Engagement decisions are admin-only — accepting an oppdrag
  commits the firm to a client. Everything administrative answers 404,
  not 403, to non-admins.
- **The firm can never lose its last active admin** — demotion and
  deactivation check inside the transaction, after locking the member
  rows (two concurrent demotions must not both see "there is another
  admin"). Memberships are deactivated, never deleted.
- **`firm_member_change` is the insert-only trail** of who let whom in
  (`registrering`/`invitasjon`/`admin`), the question a revisor asks.

Per-client assignment (which ansatte see which clients) is deliberately
not built: every active member sees every client, which is right for the
small byråer we onboard first. When a pilot byrå needs narrower scopes,
that becomes an explicit follow-up — the schema does not have to change
for it (a scoping table composes on top).

Endpoints: `GET/PUT/DELETE /firms/{fid}/access…`,
`GET/POST/DELETE /firms/{fid}/invitations…` (+ `/resend`). Portal: the
Medlemmer card in the Byrå view, visible to admins.

## The engagement flow (oppdrag)

The loop that makes it a marketplace (`docs/portal.md`: Oppdrag section
+ Byrå view):

1. **Directory** (`GET /directory/firms`): only firms with verified
   autorisasjon are listed, with an honest size signal (active client
   count).
2. **Request** (`POST /companies/{id}/engagement-requests`): requires
   company **admin**; one pending request per firm/company/kind (unique
   partial index); refused when an active engagement already exists. The
   kind is the firm's kind — requests are an audit trail, never edited
   beyond their one decision.
3. **Decision** (`POST /firms/{fid}/requests/{rid}/decision`): firm
   members only. Accepting opens the engagement in the same transaction
   as the status flip — the accountant's access exists on their next
   request, no re-login (the authorization model resolves engagements
   live).
4. **End** (`POST /companies/{id}/engagements/{eid}/end`): company admin
   sets `valid_to` = today. **`valid_to` is exclusive in access
   resolution**: ending an oppdrag revokes the firm's access
   immediately, and the history row stays forever.

## Where it is tested

- `regnmed-core/src/orgnr.rs` — MOD11 checksum on real orgnrs.
- `regnmed-gov` — registry response parsing (tolerant to extra fields),
  license matching incl. inactive licenses not counting.
- `regnmed-api/tests/grupper/engagement.rs` (real Postgres, also CI): the whole
  loop — directory listing, non-admin refused, request + duplicate
  rejection, firm-member-only visibility and decision, access appearing
  via `/me` after accept and disappearing immediately after end.
- `regnmed-api/tests/grupper/marketplace.rs` (real Postgres, mocked registries
  via env URLs, also CI): preview, checksum rejection, onboarding with
  seeded reskontro-flagged kontoplan and creator-as-admin, double
  onboarding and slettet enhet refused (for firms too, before the
  autorisasjon gate), firm creation refused without autorisasjon and
  recorded with it.
- `regnmed-api/tests/grupper/byramedlemmer.rs` (real Postgres, also CI):
  re-registration of an existing firm refused without side effects,
  invitation → redemption in `/me` → client access in the same
  response, ansatt refused (404) on member administration and
  engagement decisions (with the 400-vs-404 bogus-id probe pinning that
  the status comes from the guard), last-admin demotion/deactivation
  refused, the change trail, and a revoked invitation granting nothing.

Browser-verified against the **live** Enhetsregisteret: lookup of
974760673 showed the registry facts in the portal, and onboarding
created the company with 10 seeded accounts.
