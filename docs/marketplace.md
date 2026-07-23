# Marketplace: onboarding from the official registries

The marketplace's trust starts at onboarding: **names and facts come
from the registries, never from user input**, and firms may only offer
services after their autorisasjon is verified.

## Company onboarding (Enhetsregisteret)

`POST /companies {orgnr}` (portal: "Nytt selskap fra Enhetsregisteret"):

1. Orgnr is checksum-validated (MOD11, `regnmed-core::orgnr`) before any
   lookup.
2. Facts are fetched from BRREG's open API (`regnmed-gov::brreg`;
   `BRREG_API_URL` overrides for tests/mirrors). Slettede and
   konkurs-registrerte enheter are refused.
3. The company is created with the **registry name**, the onboarding
   person becomes admin, and a starter NS 4102 kontoplan (10 accounts)
   is seeded with 1500/2400 flagged as kunde-/leverandør-reskontro —
   invoice-ready from the first minute. An orgnr can only be onboarded
   once.

`GET /registry/enheter/{orgnr}` previews the facts (incl. autorisasjon
flags) before anything is created.

## Firm verification (Finanstilsynets register)

`POST /firms {orgnr, kind}` creates a regnskapsfører-/revisorfirma
**only** when the orgnr holds an active autorisasjon of that kind:

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
and only the adapter may move. Re-verification cadence (licenses can be
revoked) is decided together with the directory work.

## Where it is tested

- `regnmed-core/src/orgnr.rs` — MOD11 checksum on real orgnrs.
- `regnmed-gov` — registry response parsing (tolerant to extra fields),
  license matching incl. inactive licenses not counting.
- `regnmed-api/tests/marketplace.rs` (real Postgres, mocked registries
  via env URLs, also CI): preview, checksum rejection, onboarding with
  seeded reskontro-flagged kontoplan and creator-as-admin, double
  onboarding and slettet enhet refused, firm creation refused without
  autorisasjon and recorded with it.

Browser-verified against the **live** Enhetsregisteret: lookup of
974760673 showed the registry facts in the portal, and onboarding
created the company with 10 seeded accounts.
