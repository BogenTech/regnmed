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
post-logout URI. The portal derives both from `location.origin`, so the
**port is part of the registration** — moving the dev server means
re-registering the client.

- Cluster: `api.regnmed.localhost`, seeded by `scripts/dev-cluster.sh`.
- Two-process dev (no cluster): `scripts/dev-sso.sh` registers the
  client and a dev admin against a local regnid, defaulting to
  regnmed-api on **8082** and regnid on 8081. 8080 is avoided because
  colima/k3s port forwards commonly hold it. Start regnid first —
  regnmed-api resolves OIDC discovery at startup and exits if the
  issuer is unreachable.

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

## PWA: kvitteringsfoto og attestering på farten (#48)

De to tingene folk faktisk gjør på telefon er å ta bilde av en
kvittering i det den finnes, og å godkjenne noe mens de er ute.
Begge deler er den samme portalen — ingen app-butikk, ingen andre
kodebase.

**Installerbar**: `/manifest.webmanifest` (standalone, norsk, ikoner
192 og 512 — den siste `maskable`, fordi Android beskjærer) og
`/sw.js`, servert fra binæren som alt annet. Ikonene genereres av
`scripts/build-icons.py` (hand-rolled PNG, ingen bildebibliotek i
treet) og sjekkes inn ferdig, akkurat som `app.css`.

**Service workeren cacher app-skallet — og bare det.**

> Hovedboken caches aldri. Et regnskap som viser gamle tall offline er
> verre enn et som sier at det ikke har kontakt.

Skallet hentes nett-først (en oppdatert app vinner alltid), og cachen
trår bare til uten dekning. Endrende forespørsler går aldri gjennom
arbeideren.

**Kvitteringsfoto**: knappen bruker
`<input type="file" accept="image/*" capture="environment">` — kamera
på telefon, vanlig filvelger på maskin — og laster opp til det samme
innboks-endepunktet som før. API-et er uendret.

**Offline-kø, bare for opplastinger.** Uten dekning legges bildet i
IndexedDB og sendes automatisk når nettet er tilbake (ved `online` og
ved oppstart). Bildet hashes i telefonen og sendes med `?sha256=`, og
serveren:

- avviser innhold den **allerede har** («nøyaktig dette dokumentet
  ligger allerede i innboksen som …»), så en kø som prøver igjen fordi
  svaret aldri kom fram ikke gjør ett bilag til to, og
- avviser en hash som ikke stemmer med bytene den mottok, som betyr at
  bildet ble skadet underveis.

Køen fjerner et bilde når serveren avviser det (det blir ikke bedre av
å prøve igjen) og lar det ligge når det er nettet som svikter.

**Responsivt**: menyen legger seg vannrett over innholdet under `sm`
og er sidestilt fra `sm` og opp; kortkroppene ruller sine egne brede
tabeller i stedet for å dytte siden sidelengs. Temakontrakten er
urørt — samme daisyUI-temaer som på skrivebordet.

Bevisst utenfor: native apper, push-varsler (e-post dekker frister),
og offline bokføring — aldri.

## Verified

Full browser round-trip against the dev servers: SSO login via regnid →
company picker from `/me` → dashboard with live ledger numbers → customer
created → invoice issued (KID shown) → mva-spesifikasjon reflecting the
new invoice → theme switch (corporate) applied instantly.

PWA-en er verifisert i mobilviewport (375×812): service workeren
registrert og aktiv med rot-scope, manifestet lest, kameraknappen med
`capture="environment"`, menyen vannrett, ingen vannrett overflyt — og
et «kamerabilde» lastet opp gjennom den ekte veien, der den samme
filen sendt en gang til ble avvist av serveren.

`regnmed-api/tests/pwa.rs` dekker det som må stå: manifest og ikoner
serveres med riktig content-type (og er ekte PNG-er i riktig
størrelse), arbeideren nevner ikke hovedboken, `index.html` peker på
manifestet, og den samme kvitteringen to ganger blir ett bilag mens en
gal hash avvises.
