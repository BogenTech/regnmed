# The portal

The web frontend — the product surface for regnskapsførere, revisorer
and businesses. Deliberately frugal: a static single-page app with **no
JS framework and no JS build step** (Tailwind builds only the CSS),
embedded into the regnmed-api binary with `include_str!` and served on
the API's own origin. One service, no CORS, nothing extra in the
distroless image.

## Auth

OIDC authorization code + **PKCE** against regnid. The code→token
exchange is proxied through `POST /auth/token` (server-to-server), so
the IdP needs no browser CORS and the SPA never talks cross-origin.
Tokens live in sessionStorage; a 401 sends the user back to login.
Logout goes through regnid's `end_session` with `id_token_hint`
(RP-initiated logout). regnmed still never sees a password.

The public client `regnmed-portal` must have the portal's origin
registered: `{origin}/callback` as redirect URI and `{origin}/` as
post-logout URI (dev: 127.0.0.1:8080 and localhost:8080; cluster:
api.regnmed.localhost — seeded by `scripts/dev-cluster.sh`).

## Theming (the cross-site contract)

`ui/themes.css` is a **copy of the canonical `../regnid/ui/themes.css`**
— same daisyUI theme names and blocks, so a user's theme feels identical
on both sites. Update both files together. The preference is stored per
site in localStorage (`regnmed-theme`), resolved stored → system →
light, applied pre-paint by `theme.js`; it is never synced through the
IdP or tokens. Build CSS with `scripts/build-css.sh`; the generated
`crates/regnmed-api/portal/app.css` is checked in so cargo never needs
Node.

## Sections (all backed by the existing engagement-guarded API)

Oversikt (nøkkeltall + siste bilag) · Faktura (opprett, liste,
kreditnota) · Reskontro (parter, saldo, åpne poster) · Mva (spesifikasjon
per termin, mva-melding- og SAF-T-nedlasting) · Bank (camt.053-opplasting,
avstemming, manuell kobling) · Bilag (vedlegg opp/ned med sha256) ·
Periode (lås, historikk).

Authorization is entirely server-side — the portal is a *view*; a
revisor's read-only access or a stranger's 404 comes from the API, never
from hidden buttons.

## Verified

Full browser round-trip against the dev servers: SSO login via regnid →
company picker from `/me` → dashboard with live ledger numbers → customer
created → invoice issued (KID shown) → mva-spesifikasjon reflecting the
new invoice → theme switch (corporate) applied instantly.
