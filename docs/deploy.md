# Deployment

One topology, described once, deployed as overlays:

```
deploy/base    the shared manifests: Postgres 18, NATS, regnid (+ mail
               worker), regnmed-api (migrate init container, nightly
               anchor CronJob)
deploy/shared  what the SERVED environments run and the laptop does not:
               nightly backups + weekly restore-verification, and
               abonnement invoicing. Referenced by prod and test, copied
               into neither.
deploy/local   k3d in colima, *.localhost, no TLS, local-path storage —
               the integration proving ground (scripts/dev-cluster.sh,
               2 GB VM)
deploy/test    prod-shaped: replicated storage, secrets out of git, every
               scheduled job running, and the card rail against Stripe
               TEST mode
deploy/prod    real domains, TLS, secrets out of git, backups with
               restore-verification, TSA-witnessed anchoring
```

**Test is prod-shaped, not local-shaped.** That is the whole point of
it: replicated storage, the three-host split, the restore drill and the
card rail all run there, so they are found broken while that is cheap.
An environment that skips them tests nothing about them. What it does
*not* share with production: one API replica instead of two, small
volumes, and no external TSA witness — a timestamp is a claim about a
real ledger, and test roots are not that.

**Stripe runs in test mode there**, with dummy cards, so the chain that
has only ever run in integration tests — invoice issued, card charged
off-session, webhook posting the payment and closing the reskontro item
— runs against the real Stripe API. The keys come from a
`stripe-credentials` secret created out-of-band: docs/secrets.md is
absolute, and `sk_test_…` is still a key.

Det ga full uttelling 2026-07-31: den første ekte leveransen fra Stripe
avslørte at `invoice.subscription` er FJERNET i API-versjon
2026-05-27.dahlia (feltet ligger nå under
`parent.subscription_details`). Webhooken svarte 200, Stripe førte
leveransen som vellykket — og fakturaen ble aldri til. Ingen
integrasjonstest kunne fanget det: testens payload var skrevet ut fra
hva koden leser, så den bekreftet kodens egen antakelse. Samme dag
avdekket en oppsigelse på tegningsdagen at dekningsraden ikke kunne
lukkes (`valid_to > valid_from`), slik at selskapet beholdt tilgangen
gratis. Begge feilene var USYNLIGE ovenfra — derfor kjøres ekte trafikk
mot testmiljøet før produksjon, og derfor kontrolleres hovedboken
etterpå, ikke svarkoden.

`kubectl kustomize deploy/<overlay>` renders any of them; the restructure
kept the local render byte-identical, so dev-cluster.sh is unchanged.

## Utrulling: clusteret HENTER, GitHub dytter aldri

| Miljø | Følger | Utløses av |
| --- | --- | --- |
| test | grenen `main`, `deploy/test` | hver push **med grønn CI** |
| prod | semver-tagger `>=0.1.0`, `deploy/prod` | at en release publiseres |

**CI-porten (2026-08-06):** `images`-workflowen og `ci`-workflowen
kjører parallelt, og rullesteget i images ventet ikke — test fikk to
ganger et bygg hvis CI senere feilet. Rullesteget venter nå på at `ci`
er grønn på samme commit før test-overlayet skrives; feiler CI, feiler
images-jobben på samme sted, synlig. Selve image-pushen er ikke portet
(et image i registeret deployer ingenting). Forhåndsutgaver
(`vX.Y.Z-rcN`) er fortsatt fluktluken for risikofylte endringer: de
bygger et image utenfor prods semver-område og kan soakes på test før
den ordentlige taggen kuttes.

Flux kjører i clusteret og henter fra git. Manifestene for det ligger i
søsterrepoet `../homelab` (`cluster/flux-regnmed.yaml`) — de hører ikke
hjemme her, like lite som UniFi-manifester gjør.

**Retningen er ikke en smakssak.** To forhold tvinger den, og begge de
enklere alternativene er feil:

1. k3s-API-et ligger bak NAT, så en GitHub-hostet runner kan ikke nå det
   — og løsningen er *ikke* å åpne 6443 mot internett, for det er å
   publisere et cluster-admin-endepunkt.
2. **BogenTech/regnmed er et OFFENTLIG repo.** En self-hosted runner
   ville da la hvem som helst som åpner en pull request kjøre kode på en
   maskin inne i hjemmenettet, ved siden av hovedboken. GitHub anbefaler
   selv mot nettopp dette.

### Hvor hemmelighetene bor

**I clusteret, og ingen andre steder.** `db-credentials` og
`stripe-credentials` opprettes én gang med `kubectl create secret` og
lever videre gjennom hver utrulling. CI trenger dem aldri: den bygger et
image og skriver en tagg til git, og clusteret gjør selve applyen.
Derfor kan en kompromittert workflow på et offentlig repo ikke nå
Stripe-nøklene eller databasen.

Ingenting legges i GitHub repo secrets eller environment secrets. Flux
trenger heller ingen deploy key, siden repoet er offentlig og lesing er
anonym. Dette er docs/secrets.md som holder, ikke som omgås.

`.env` er utelukkende for lokal utvikling (se `.env.example`, som
dokumenterer alle 28 variablene) og har ingen rolle i utrulling.

### Hvordan en test-utrulling faktisk skjer

`.github/workflows/images.yml` bygger og pusher
`ghcr.io/bogentech/regnmed:sha-<short>`, og **committer så den taggen inn
i `deploy/test/kustomization.yaml` på main. Den commiten ER utrullingen**
— Flux rekonsilierer den innen minuttet. Git blir den eneste kilden til
sannhet, og taggene forblir uforanderlige, som er nettopp det som gjør at
test alltid kan navngi commiten den kjører.

Workflowen har `paths-ignore: deploy/**`. Uten den ville dens egen
tagg-commit utløst et nytt bygg, i det uendelige.

### Å slippe til produksjon

Produksjon er **automatisk ved release, uten godkjenningssteg** — releasen
er den bevisste handlingen. To følger av det:

- Commiten du tagger må ALLEREDE peke på det image-taggen prod skal
  kjøre, i `deploy/prod/kustomization.yaml`. Prod følger ikke nyeste
  image; den følger det releasen sier. Det er dette som gjør at prod kan
  navngi sin egen commit.
- Å slette en feil tagg ruller ikke tilbake. Flux går til den høyeste
  semver-taggen som matcher, så publiser en rettet, høyere release i
  stedet.

**Slippflyten** (revidert 2026-08-06 — versjonen i brukermenyen skal
være releasen, og hver release skal forklare seg):

1. Verifiser at **testmiljøet faktisk kjører** shaen du slipper —
   `test.regnmed.no` serverer portalbunten fra det bygget (CI ruller
   test automatisk ved push til main).
2. Sett `newTag: vX.Y.Z` (den KOMMENDE taggen) for regnmed i
   `deploy/prod/kustomization.yaml`, commit («Prod: roll to vX.Y.Z»),
   **tagg samme commit** `vX.Y.Z`, push begge. Tag-pushen bygger imaget
   med `REGNMED_VERSION=vX.Y.Z` bakt inn — Flux kan stå i
   ImagePullBackOff i ~4 minutter til imaget finnes, og henter seg inn
   selv. Kodeekvivalensen sha == tag holder fordi pin-commiten bare
   rører `deploy/`, som aldri går inn i imaget.
3. **Opprett GitHub-releasen** — taggen alene er ikke nok:

   ```sh
   gh release create vX.Y.Z --title "vX.Y.Z" --notes "$(git log --format='- %s' vFORRIGE..vX.Y.Z^)"
   ```

   Notatene skal si hva som slippes siden forrige release, i klartekst —
   det er endringsloggen kunder og drift leser, og `git log`-linjene er
   utgangspunktet, ikke fasiten: rediger til noe et menneske har glede
   av.
4. Verifiser at prod svarer med releasen:
   `curl -s https://regnmed.no/portal-config` skal bære
   `"versjon": "vX.Y.Z"`.

Forhåndsutgaver (`v1.2.3-rc1`) er utenfor semver-området, så en
release-kandidat kan tagges og inspiseres uten å røre produksjon.

`prune: true` på begge: fjerner du en ressurs fra et overlay, blir den
faktisk slettet i clusteret. Uten det samler det seg opp levende objekter
ingen manifest beskriver — det skjedde her én gang alt, med en
gjenglemt ClusterIssuer.

## Production checklist (deploy/prod)

1. **Pin images.** Build with `scripts/build-images.sh`, push to your
   registry, set the two `newTag` values in
   `deploy/prod/kustomization.yaml`. Never `:dev` in production.
2. **Hosts.** Three of them, and they mirror the local cluster:

   | Host | Serves | Ingress |
   | --- | --- | --- |
   | `regnmed.no` | the portal — what a human opens | `regnmed-portal` |
   | `api.regnmed.no` | the API — what an integration calls | `regnmed-api` |
   | `id.regnmed.no` | the IdP | `regnid` |

   The first two point at the **same service**: one binary serves both
   the SPA and the API (docs/portal.md). That is deliberate, and it is
   what keeps the portal's own calls same-origin — the app asks for
   `/companies/…` relative to wherever it was loaded, so a human on
   `regnmed.no` never makes a cross-origin request. No CORS, and
   `connect-src 'self'` in the CSP (docs/auth.md §9) keeps meaning what
   it says. Split them onto different services and both stop holding.

   Edit the hostnames in `deploy/prod/ingress.yaml`, the matching
   `ISSUER`/`OIDC_ISSUER` values in `deploy/prod/patches/` — the issuer
   URL the browser sees must be the one the pods see — and
   `PORTAL_BASE_URL`, which is the **portal** host, since it becomes the
   link in the invitation e-mail (#66). Register `https://regnmed.no/callback`
   as a redirect URI on the portal's OIDC client; add the API host too if
   the app should be openable there as well.
3. **Volumes — replicated storage, and check the arithmetic.**
   Everything about volumes lives in `deploy/prod/patches/storage.yaml`,
   which shows its working.

   **Production needs a replicated storage class.** The base manifests
   name none, so the local k3d cluster gets `local-path` — a directory
   on the single node. That is right for a laptop and wrong for
   production, where a node-local volume means the database dies with
   its node. The prod patch names `longhorn`; any distributed
   provisioner does (Rook/Ceph, OpenEBS Mayastor, a cloud class), and it
   is named in that one file so switching is one edit. What the class
   must provide, whatever its name: replication across nodes,
   `allowVolumeExpansion: true`, and snapshots.

   `ReadWriteOnce` is correct and stays — Postgres is a single writer,
   and the two CronJobs sharing `/backup` never run at once (01:30
   nightly, 04:00 Sundays), so the volume reattaches cleanly if they
   land on different nodes.

   **The local cluster deliberately does not run Longhorn.** It needs
   iSCSI, real block devices and roughly a gigabyte for its own
   components; `dev-cluster.sh` fits in a 2 CPU / 2 GB VM on purpose
   (docs/frugality.md). So this is the one place where local stops
   mirroring production, and it is worth knowing before a storage
   problem is met for the first time in prod. Everything above the
   volume — schema, jobs, restore drill — is identical.

   Sizes: 50Gi for Postgres and 150Gi for backups (the base sizes are
   for the local VM).

   **The two are not independent.** The nightly job keeps 14
   `pg_dump -Fc` files, and `-Fc` compression does nothing for content
   that is already JPEG or PDF — so the backup volume needs
   `retention × database`, not some fraction of it. The pair that
   shipped before this was already broken in that direction: 2Gi of
   database against 10Gi of backups, where a full database needs ~28Gi
   of dumps. **The backup volume would have filled first**, and the
   nightly job would have begun failing while the database still looked
   healthy. Failed Jobs are the alerting signal (see Observability), so
   it would have been visible — but the thing that broke would not have
   been the thing that was undersized.

   What fills the database is not the ledger; vouchers and entries are
   small rows. It is four bytea columns — bilagsvedlegg, innboks
   documents, receipt photos, raw e-mail — capped at 20 MB per upload
   and averaging a few hundred KB in practice.

   Full daily dumps do not scale with blob storage: they re-copy every
   unchanged photo, nightly. PITR via CloudNativePG (below) is the
   answer, and the trigger to adopt it is the database passing ~20 GB,
   not a date.

   **Replication multiplies all of it.** Those are the sizes Kubernetes
   reports; a replicated class stores each one N times, and Longhorn's
   default N is 3 — so 50 + 150 + 1 Gi of volumes is about **600Gi of
   raw disk** across the pool. That is the number to provision against.
   It is also the reason to consider `numberOfReplicas: 2` for the
   backup volume specifically: its contents are already a copy, and a
   third replica of a copy is thin value for 150Gi.

   **Storage snapshots do not replace the dumps.** A block snapshot of a
   running Postgres is crash-consistent — Postgres recovers from it as
   from a power cut — but it proves nothing about the ledger. The weekly
   restore-verification exists to prove exactly that, and it stays
   whatever the storage layer offers. Treat snapshots as a faster
   recovery tier layered on top.

   Volumes can usually be grown in place later
   (`allowVolumeExpansion: true` on the StorageClass) but never shrunk.
   Confirm that before the first apply; the alternative is
   dump-and-restore.
4. **TLS — terminert av Traefik i clusteret (fra 2026-08-01).** Dette er
   den ene plassen som sier hvilket hopp som eier sertifikatene, og den
   må oppdateres i samme endring som manifestene.

   I dag: **Traefik** eier ruterens 80/443, og **cert-manager** utsteder
   Let's Encrypt-sertifikater per ingress via HTTP-01. Hver regel har
   `tls:` og `cert-manager.io/cluster-issuer: letsencrypt`.
   `ClusterIssuer`-objektet er clusteromfattende og bor derfor i
   hjemmelab-repoet (`stacks/edge/`), ikke her — to repoer som begge
   eier samme objekt ville kjempet om det. `cert-issuer.yaml` blir
   liggende i overlayene som klargjort konfigurasjon for en frittstående
   utrulling som ikke har hjemmelab-repoet.

   HISTORIKK, og grunnen til at overgangen måtte gjøres i to trinn:
   fram til 2026-08-01 terminerte **nginx-proxy-manager** TLS og
   videresendte ren HTTP til Traefik. Ingressene hadde da verken
   `tls:`-blokk eller issuer-annotasjon, og det var ikke latskap: en
   ruter videresender 80/443 til ÉN destinasjon og kan ikke rute på
   vertsnavn — det er L7, og en ruter er L4. Ba man cert-manager om
   sertifikater mens NPM eide porten, kunne det ikke fungere: NPM svarte
   på HTTP-01-utfordringen i stedet for solver-poden. Erfart 2026-07-29,
   tre `Certificate`-objekt sto `ready=False` med «failed to perform
   self check». Sertifikatet var ikke problemet — topologien var det.

   SELVE OVERGANGEN ble derfor delt: **port 80 flyttet til Traefik
   først**, mens 443 fortsatt gikk til NPM. Da kunne cert-manager løse
   HTTP-01 og utstede alle sertifikatene mens HTTPS var uforstyrret, og
   443 ble flyttet først da hvert `Certificate` sto `Ready`. Grunnen til
   å ikke bare flytte begge: Let's Encrypt tillater **5 mislykkede
   valideringer per vertsnavn per time**, så å rulle ut annotasjonene før
   porten var flyttet ville brent kvoten nøyaktig når man trenger den.

   Prisen, fortsatt verdt å kjenne: produksjons-ACME har grenser **per
   registrert domene**, så `test.regnmed.no` og `regnmed.no` deler
   budsjett. cert-manager fornyer på 2/3 av levetiden og reutsteder ikke
   ved hver ombygging, men et cluster som stadig gjenskapes kan komme
   borti taket.

5. **Secrets — before the first apply, never in git:**

   ```sh
   kubectl -n regnmed create secret generic db-credentials \
     --from-literal=password='<strong password>' \
     --from-literal=regnmed-url='postgres://regnmed:<pw>@postgres:5432/regnmed' \
     --from-literal=regnid-url='postgres://regnmed:<pw>@postgres:5432/regnid' \
     --from-literal=restore-check-url='postgres://regnmed:<pw>@postgres:5432/regnmed_restore_check'
   ```

   Kortskinnen (#74) trenger sin egen, samme regel — aldri i git:

   ```sh
   kubectl -n regnmed create secret generic stripe-credentials \
     --from-literal=secret-key='sk_live_…' \
     --from-literal=webhook-secret='whsec_…'
   ```

   Webhook-hemmeligheten er den **live**-endepunktets: Stripe utsteder
   én per modus, og en test-hemmelighet her gjør at hver eneste ekte
   hendelse avvises som usignert.

   **I PRODUKSJON er kortskinnen foreløpig AV** (besluttet 2026-07-31 ved
   første utrulling): `STRIPE_*` er tatt ut av
   `deploy/prod/patches/regnmed-api.yaml`, så `stripe-credentials`
   trengs ikke der ennå og portalen sier at kortbetaling ikke er
   tilgjengelig. Å slå den på er de to blokkene som står igjen i
   toppkommentaren i patchen, pluss hemmeligheten — og da med
   **live**-verdier. En testnøkkel der ville ikke feilet: den ville tatt
   imot et ekte kort, ikke trukket noe, og likevel lukket
   reskontroposten.

   **I TESTMILJØET må BEGGE hemmelighetene finnes FØR første apply**
   (`-n regnmed-test`, med `sk_test_…`/`whsec_…` fra Stripes testmodus).
   Mangler `stripe-credentials` mens patchen krever den, starter ikke
   regnmed-api i det hele tatt: poden blir stående i
   `CreateContainerConfigError` med «secret "stripe-credentials" not
   found», mens Postgres, migrasjonene og alt annet ser friskt ut.
   Feilen står i pod-hendelsene, ikke i loggene — containeren rekker
   aldri å starte. Erfart 2026-07-29 ved første utrulling av
   testmiljøet.

   Every DATABASE_URL/POSTGRES_PASSWORD/STRIPE_* in the prod render
   comes from a secret; the rendered YAML contains no credential
   (usernames and the OIDC audience are the only literals).
6. **Abonnementsfakturering og kortskinnen** (docs/abonnement.md):
   onboard driftsselskapet i regnmed (BRREG, som alle andre) og sett
   dets orgnr som `REGNMED_DRIFT_ORGNR` **begge stedene** — på
   `regnmed-api` (patches/regnmed-api.yaml) og på CronJob-en
   (`deploy/shared/abonnement-faktura.yaml`, patchet i overlayet).

   **De tre kortinnstillingene er et sett.** `STRIPE_SECRET_KEY` og
   `STRIPE_WEBHOOK_SECRET` er begge-eller-ingen — mangler én er skinnen
   av, og endepunktene sier det. Den tredje, `REGNMED_DRIFT_ORGNR`, er
   den som svir: webhooken er det som gjør et Stripe-trekk om til et
   betalingsbilag og en lukket reskontropost, og uten orgnr vet den ikke
   hvilken hovedbok det hører hjemme i. Stripe har uansett tatt pengene.
   **Å sette de to første uten den tredje er verre enn å la skinnen være
   av**: kunden trekkes, og regnskapet får det aldri med seg. Og de to
   stedene må ha SAMME orgnr — ellers utstedes fakturaen i én hovedbok
   og betalingen bokføres i en annen, og posten lukkes aldri.

   Pek Stripes live-webhook på `https://<api-host>/webhooks/stripe`.
7. **E-post inn** (#35, docs/epost-inn.md): `MAIL_IN_DOMAIN` er vanlig
   konfigurasjon, ikke en hemmelighet — domenet innboksadressene vises
   under. Sett det **først når det domenets MX faktisk leverer inn i
   regnids mottak**; ellers viser portalen en adresse som stille tar
   imot ingenting, og det er nettopp det den usatte tilstanden finnes
   for å unngå.
8. `kubectl apply -k deploy/prod`.
9. **Bootstrap** — OIDC-klienten og det første mennesket, se neste
   avsnitt. Uten steget står et miljø oppe som ingen kan logge inn i.

## Bootstrap etter første apply

Manifestene reiser topologien, men de kan ikke reise **staten inne i
regnid**. OIDC-klienten og den første kontoen er rader i regnids
database, og et passord kan uansett aldri ligge i dette repoet
(docs/secrets.md). Da finnes bare én plass framgangsmåten kan bo, og det
er her.

Den bodde ingen steder fram til 2026-08-06, og prisen kom som forventet:
spørsmålet «hvilken administrator lagde vi til testmiljøet?» kunne ikke
besvares av noen, og repoet kunne ikke svare heller — det eneste
`add-user`-kallet i git er `seed()` i `scripts/dev-cluster.sh`, som
sår **det lokale** clusteret og aldri har rørt `test.regnmed.no`.
Passordet hører hjemme i en passordhåndterer; det som mangler her er
bare hvilke steg som ble tatt.

Stegene kjøres én gang per miljø, etter første apply. `-n regnmed-test`
for test, `-n regnmed` for prod; alt annet er likt. Eksemplene under er
testverdier — bytt vertsnavnene i prod.

### 1. OIDC-klienten `regnmed-portal`

```sh
kubectl -n regnmed-test exec deploy/regnid -- /app/regnid add-client \
    --client-id regnmed-portal --name "regnmed portal" \
    --redirect-uri https://test.regnmed.no/callback \
    --redirect-uri https://api.test.regnmed.no/callback \
    --post-logout-redirect-uri https://test.regnmed.no/ \
    --post-logout-redirect-uri https://api.test.regnmed.no/ \
    --audience regnmed
```

**`--audience regnmed` er det som svir hvis den glemmes.** Innloggingen
virker helt fint uten den — brukeren kommer tilbake til portalen med et
token — og så svarer hvert eneste API-kall 401, uten et spor noe sted
som peker på klientregistreringen. Erfart i prod 2026-07-31.

Klient-id-en må være den API-et ber om: `PORTAL_OIDC_CLIENT_ID`, som
faller tilbake på `regnmed-portal` (`crates/regnmed-api/src/portal.rs`).
Redirect-URI-ene er **hver origin portalen kan åpnes fra**, ikke bare
den vanlige: appen bygger sin egen `redirect_uri` av `location.origin`
(`ui/portal/src/lib/auth.svelte.js`), og de to første vertsnavnene peker
på samme service — én binær serverer både SPA-en og API-et — så portalen
svarer også på API-verten. Åpner noen den der, må den origin være
registrert, ellers avviser regnid redirecten.

### 2. Den første kontoen

```sh
kubectl -n regnmed-test exec deploy/regnid -- /app/regnid add-user \
    --email deg@example.no --password '<engangspassord>' \
    --name 'Navn Navnesen' --admin
```

`--admin` gjelder **regnids egne administrasjonssider** (brukere og
klienter i IdP-en) og gir ingen tilgang til noen hovedbok — grensen i
docs/auth.md §8 går ikke gjennom regnid. Passordet står i klartekst i
kommandoen, altså i skallhistorikken: bruk et engangspassord og bytt det
fra kontosiden etter første innlogging. Regnid har også `/forgot`, men
den veien forutsetter at utgående e-post faktisk leveres i miljøet
(`MAIL_BACKEND=nats` + en mail-worker med transport-legitimasjon) — er
den ikke oppe, er `add-user` eneste vei inn.

### 3. Selskapsadministrator lages ikke her

Den kommer av seg selv: den som onboarder et selskap fra
Enhetsregisteret blir dets administrator (docs/marketplace.md).
Registreringsflyten i portalen er hele framgangsmåten, og det finnes
ingen CLI-vei — med vilje.

### 4. Plattformrolle, hvis miljøet skal ha en

```sh
kubectl -n regnmed-test exec deploy/regnmed-api -- /app/regnmed platform-grant \
    --epost deg@example.no --rolle systemadmin \
    --til 2026-12-31 --notat 'Sak 123: drift av testmiljøet'
```

Personen **må ha logget inn i portalen én gang først** — `platform-grant`
slår opp en eksisterende `person`, og finner den ingen, sier den det.
`--til` er obligatorisk og eksklusiv (docs/auth.md §8: en plattformrolle
uten utløpsdato kan ikke skrives inn). `platform-list` viser
medlemskapene, `platform-end --id <id>` avslutter et med virkning fra
neste kall.

### Hva finnes allerede i et miljø?

Det er dette spørsmålet avsnittet finnes for. Kontoene:

```sh
kubectl -n regnmed-test exec deploy/postgres -- psql -U regnmed -d regnid -c "select email, name, is_admin, created_at from users order by created_at"
```

(`psql` over containerens egen socket trenger ikke passord.) Og
plattformrollene, aktive som avsluttede:

```sh
kubectl -n regnmed-test exec deploy/regnmed-api -- /app/regnmed platform-list
```

## Pod hardening

Every container runs unprivileged, with no capabilities and a default
seccomp profile: `allowPrivilegeEscalation: false`,
`capabilities: drop [ALL]`, `seccompProfile: RuntimeDefault`. Beyond
that, the containers differ, and the differences are the interesting part.

| | runs as | root fs |
| --- | --- | --- |
| regnmed-api, regnid, mail worker, every CronJob | 65532 (nonroot) | read-only |
| postgres | 70 (`postgres`) | writable |
| nats | 65532 | read-only |

Our own images are distroless `:nonroot`, so they already had a nonroot
USER — but **an image saying so is not the same as the cluster requiring
it**, and nothing was requiring it. `runAsNonRoot` is what makes a future
image change fail loudly instead of quietly running as root.

`postgres` and `nats` were genuinely running as **root** until this was
set; both images drop privileges themselves, or would have if asked, and
neither was being asked. Postgres needs `fsGroup: 70` so the data
directory is owned by the user it now runs as, and keeps a **writable
root filesystem** on purpose: it writes its socket to
`/var/run/postgresql` and temp files to `/tmp`, neither a volume.
Turning those into emptyDirs to win the flag would add moving parts for
no real gain — the container is already unprivileged and capability-free.

`nats` gained `-m 8222` and a readiness probe on `/healthz`. Without one,
a client could be routed to a NATS that had not finished opening its
JetStream store. The mail worker deliberately has **no** probe; the
manifest explains why.

## Backups — restored weekly, or they don't count

`deploy/shared/backup.yaml` (prod and test both run it):

- **Nightly** `pg_dump` (custom format) of both databases to the
  backup PVC, pruned after 14 days.
- **Weekly restore-verification**: the newest dump is restored into a
  scratch database and `regnmed verify-ledger` re-walks every hash
  chain **in the restored copy** — including the anchor checks. This
  proves, unattended, that the backup restores *and* that the restored
  ledger is untampered. A backup that has never been restored is a
  hope, not a backup.

The same drill runs anywhere via `scripts/backup-verify.sh`
(`DATABASE_URL=… scripts/backup-verify.sh`). It has been exercised both
ways: a clean ledger passes; a database containing forged anchor rows
fails with the tampering named. Copy the backup PVC off-cluster (object
storage, another site) — a backup next to its database shares its
fate.

**Growth path, deliberate:** when RPO of minutes (not a day) is
required, move Postgres to the CloudNativePG operator with WAL
archiving to object storage — true PITR. The dump+verify drill stays
even then; PITR replaces the nightly granularity, not the verification.

## Observability, within the frugality budget

No metrics stack by default — the budget (docs/frugality.md) is spent
on the product. What production runs on:

- **Probes**: `/health` readiness on the API; pg_isready on Postgres.
- **Integrity monitoring is the observability that matters here**: the
  nightly anchor CronJob (every root witnessed via `ANCHOR_TSA_URL`)
  and the weekly backup-verification are both *checks that fail loudly
  in `kubectl get jobs` / alerting on failed Jobs* — they watch the one
  thing this system promises, the ledger.
- **Logs**: `kubectl logs`; ship them with the cluster's collector if
  one exists. A `/metrics` endpoint is a conscious later addition, and
  the frugality gate will price it.

## Deliberately not yet

Multi-node/HA Postgres, CloudNativePG (above), NetworkPolicies,
autoscaling — added when a real load or a real requirement asks, each
priced against the frugality budget.

Longhorn in the **local** cluster is on this list too, and stays there:
it needs iSCSI, real block devices and about a gigabyte for itself,
against a 2 GB VM. The manifests support a replicated class (checklist
item 3) without the laptop having to run one.
