# The portal

The web frontend — the product surface for regnskapsførere, revisorer
and businesses. A **Svelte 5 (runes) single-page app**, built by Vite,
embedded into the regnmed-api binary and served on the API's own origin.
One service, no CORS, nothing extra in the distroless image.

## Byggekjeden: node bygger, cargo bare leser (#76)

Kilden ligger i `ui/portal/`. `scripts/build-portal.sh` kompilerer den,
og **`ui/portal/dist/` sjekkes inn** — nøyaktig presedensen den
genererte `app.css` satte. Rust-siden leser dist med `include_dir`, så
`cargo build`, og særlig kryssbygget i `build-images.sh`, **trenger
aldri Node**. Vite gir innholdshashede filnavn, derfor `include_dir` og
ikke én `include_str!` per fil.

En innsjekket artefakt lyver hvis den ikke er bygget fra kilden, så en
CI-jobb (`portal`) bygger portalen på nytt og feiler dersom `dist`
avviker. dist har dessuten sitt eget budsjett i `scripts/frugality.sh`.

Adressene:

- `/` og `/callback` → appen (rutingen skjer i hashen, og
  callback-adressen må lande i appen for å fullføre PKCE-flyten).
- `/assets/*` → Vite-filene, `immutable` og cachet for alltid fordi
  filnavnet bærer innholdshashen. En ukjent asset gir **404**, ikke
  appen på nytt: en 200 med HTML der JS-en skulle vært feiler stille.
- `/ny` → permanent redirect til `/`. Det var appens adresse mens
  migreringen pågikk; gamle bokmerker skal ikke lande i en 404.

Historikk: fram til 2026-07-29 var portalen rammeverksfri og uten
byggesteg (`app.js` + `theme.js` + generert `app.css`, `include_str!`).
Vedtaket om å bytte, og hvorfor, står i #76.

## Utvikling

```sh
scripts/build-portal.sh          # bygg dist (kjøres før commit av UI-endringer)
cd ui/portal && npm run dev      # hot reload mot en kjørende regnmed-api
```

Dev-serveren proxyer API-kallene til `localhost:8080`, så innlogging og
data virker som i produksjon.

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

- Cluster: `regnmed.localhost` (portalen) og `api.regnmed.localhost`
  (API-verten, samme binær), begge seedet av `scripts/dev-cluster.sh`.
  **Begge opphav må stå på klienten**, siden appen kan åpnes fra begge.
- Two-process dev (no cluster): `scripts/dev-sso.sh` registers the
  client and a dev admin against a local regnid, defaulting to
  regnmed-api on **8082** and regnid on 8081. 8080 is avoided because
  colima/k3s port forwards commonly hold it. Start regnid first —
  regnmed-api resolves OIDC discovery at startup and exits if the
  issuer is unreachable.

## Theming (the cross-site contract)

`ui/portal/themes.css` is a **copy of the canonical
`../regnid/ui/themes.css`** — same daisyUI theme names and blocks, so a
user's theme feels identical on both sites. Update both files together.
(The copy lives beside the app because `@plugin "daisyui/theme"`
resolves against the nearest `node_modules`, and Node lives only in
`ui/portal`.) The preference is stored per site in localStorage
(`regnmed-theme`), resolved stored → system → light, applied pre-paint
by an inline script in `index.html` so the page never flashes; it is
never synced through the IdP or tokens. Tailwind and daisyUI are built
by Vite as part of `scripts/build-portal.sh`.

**Alle daisyUIs innebygde temaer er tilgjengelige** (`themes: all` i
`ui/portal/src/app.css`, samme i `../regnid/ui/input.css`) — 35 innebygde
pluss våre to egne, `regnid` og `kontrast`. Temavalget er brukerens, og en
håndplukket liste måtte vedlikeholdes for hånd hver gang daisyUI la til et
tema. To ting å vite:

- `all` kan **ikke** kombineres med `light --default, dark --prefersdark`:
  da faller resten av listen stille bort (verifisert 2026-08-03 — 4 temaer
  i bygget i stedet for 37). `all` beholder de samme standardene selv:
  `light` på `:where(:root)`, `dark` under `prefers-color-scheme`.
- Velgeren er **gruppert etter daisyUIs egen `color-scheme`** på hver
  temablokk, ikke etter skjønn: `THEME_GROUPS` i
  `src/lib/theme.svelte.js` og `THEME_GROUPS` i regnids `src/accounts.rs`
  (som også er allowlisten `is_theme` validerer mot). En flat liste på 37
  navn er ubrukelig, og den som vil ha et mørkt tema skal slippe å gjette.

Selve velgeren er daisyUIs **theme-controller** i et `dropdown`+`menu`
(begge steder): en avkrysset radio med temanavnet som `value` velger
temaet i REN CSS. I regnid heter radioene `theme`, så skjemaet POSTer
valget uten JS i innsendingsveien. JS-en består likevel — den husker
valget (localStorage) og setter `data-theme` FØR første maling, ellers
blinker siden i feil tema.

**Fellen, og hvorfor «system» er bygget som den er:** en avkrysset
theme-controller har HØYERE spesifisitet enn `[data-theme]`
(`:root:has(input.theme-controller[value=x]:checked)` slår
`[data-theme=x]`). «Følg systemet» er derfor med vilje IKKE en
theme-controller: den ligger i samme radiogruppe, så den krysser av de
andre, og uten noen avkrysset controller finnes ingen overstyring — da
får OS-preferansen bestemme. Var «system» en controller, ville en
gjenglemt avkryssing overstyrt den i det stille.

Nye temaer lages enklest med daisyUIs **Theme Generator**
(<https://daisyui.com/theme-generator/>): den skriver ut nøyaktig én
`@plugin "daisyui/theme" { … }`-blokk, som er formatet `themes.css`
allerede har. Lim blokken inn i BEGGE `themes.css`-filene, legg navnet i
begge `THEME_GROUPS`, og bygg (`scripts/build-portal.sh` +
`../regnid/scripts/build-css.sh`).

Kostnaden er ren CSS: dist vokste ~32 KB (~21 KB gzipet) og ligger godt
innenfor `PORTAL_DIST_BUDGET_KB` i `scripts/frugality.sh`.

## Sections (all backed by the existing engagement-guarded API)

Én komponentmappe per seksjon under `ui/portal/src/sections/`, registrert
i `App.svelte` og navngitt i `lib/meny.js`:

Oversikt (stats, nøkkeltall, forankring, abonnement, firmaopplysninger,
migrering av tomt selskap) · Faktura (ny faktura, liste m/ PDF/EHF/send,
kreditnota, forfalte + purring, tilbud→ordre, repeterende) · Produkter
(register, varelager, telling) · Timer · Lønn · Utlegg · Anlegg ·
Aksjonærer · Reskontro (parter, åpne poster, CSV-import) · Mva
(spesifikasjon, eksport, terminordning) · Rapporter (saldobalanse,
resultat, balanse, budsjett/avvik, konto- og bokføringsspesifikasjon,
revisjon) · Bank (kontoutskrift, avstemming, betalingsliste) · Bilag
(attestering, innboks, e-post-inn, vedlegg, dimensjoner) · Periode ·
Oppdrag (tilgang, roller, integrasjoner) — pluss byråvisningen på
`#/byra/{id}`.

Fellesdeler ligger i `src/lib/` (API-klient, auth, ruter, tema, toast,
nedlasting, offline-kø) og `src/components/` (skall, kort, dimensjons-
og mva-velger). Ruteren leser hash-spørringen ett sted, så
`#/c/{id}/mva?year=&termin=` fortsatt kan deles.

Authorization is entirely server-side — the portal is a *view*; a
revisor's read-only access or a stranger's 404 comes from the API, never
from hidden buttons. Menyen skjuler bare det en rolle ikke kan bruke, så
ingen klikker seg inn i en feilmelding; sperren ligger i tilgangsvakten.

## PWA: kvitteringsfoto og attestering på farten (#48)

De to tingene folk faktisk gjør på telefon er å ta bilde av en
kvittering i det den finnes, og å godkjenne noe mens de er ute.
Begge deler er den samme portalen — ingen app-butikk, ingen andre
kodebase.

**Installerbar**: `/manifest.webmanifest` (standalone, norsk, ikoner
192 og 512 — den siste `maskable`, fordi Android beskjærer) og
`/sw.js`, servert fra binæren som alt annet. Ikonene genereres av
`scripts/build-icons.py` (hand-rolled PNG, ingen bildebibliotek i
treet) og sjekkes inn ferdig, akkurat som `dist`.

**Service workeren cacher app-skallet — og bare det.**

> Hovedboken caches aldri. Et regnskap som viser gamle tall offline er
> verre enn et som sier at det ikke har kontakt.

Siden skallet er Vite-bygget, kan filnavnene ikke listes opp på
forhånd. Regelen er derfor uttrykt som **adresser**: `/assets/*` er
bygde, uforanderlige filer, og de faste PWA-filene er navngitt. Alt
annet — hele API-et — går utenom cachen. Skallet hentes nett-først (en
oppdatert app vinner alltid), og cachen trår bare til uten dekning.
Endrende forespørsler går aldri gjennom arbeideren.

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

`regnmed-api/tests/grupper/pwa.rs` dekker det som må stå: manifest og ikoner
serveres med riktig content-type (og er ekte PNG-er i riktig
størrelse), arbeideren nevner ikke hovedboken, `index.html` peker på
manifestet, og den samme kvitteringen to ganger blir ett bilag mens en
gal hash avvises. `tests/grupper/portal.rs` dekker serveringen: `/` og
`/callback` gir appen, hver hashet asset serveres `immutable`, en ukjent
asset gir 404 (ikke appen på nytt), `/ny` redirecter til `/`, og de
gamle adressene `/app.js`, `/app.css`, `/theme.js` er borte.

### Flippen (2026-07-29, #76 steg 3)

Svelte-appen overtok `/`, og den rammeverksfrie portalen ble slettet i
samme endring: `app.js` (4 174 linjer), `theme.js`, den genererte
`app.css`, `index.html`, `scripts/build-css.sh` og `ui/input.css`.
Verifisert i nettleseren etterpå: appen serveres fra roten, service
workeren er registrert med rot-scope og cachen inneholder **bare**
skallet (`/`, `/assets/*`, ikoner, manifest) og ingenting fra
hovedboken, den gamle `v1`-cachen er ryddet bort, de gamle adressene
svarer 404, `/ny` redirecter, og 375×812 har fortsatt ingen vannrett
sidescroll.
