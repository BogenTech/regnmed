---
title: Selvhosting
description: Kjør regnmed selv. Hva som trengs, miljøvariablene som betyr noe, og hva AGPL krever av deg.
sidebar:
  order: 10
---

regnmed er åpen kildekode (AGPL-3.0) og kan kjøres på egen infrastruktur. Kunder av den hostede tjenesten på regnmed.no trenger ingen lisens fra oss og kan hoppe over denne siden.

:::caution[Selvhosting betyr at du bærer ansvaret]
De offentlige integrasjonene (Maskinporten, Altinn, Skatteetatens API-er) krever organisasjonens egne avtaler og nøkler. De overføres ikke med koden. Den årlige regelverksrevisjonen (satser, kodelister, skjemaer) er heller ikke din med mindre du tar den selv.
:::

## Hva som trengs

| Komponent | Krav | Merknad |
| --- | --- | --- |
| **PostgreSQL** | **18 eller nyere**, aldri lavere | Eneste tilstand. Migrasjoner kjøres av `regnmed migrate` |
| **Identitetstilbyder** | Enhver OIDC-issuer. regnmed validerer tokens og kjenner ikke IdP-en | regnmeds egen er **regnid**, et eget repo som aldri er vendored hit |
| **NATS JetStream** | Valgfri | E-post ut og inn deler én mail-rail. Uten `NATS_URL` er e-postutsendelse og e-post-inn av, og sier det tydelig i stedet for å late som |
| S3-kompatibelt lager | Valgfritt | Dokumenter (vedlegg, innboks) ligger ellers i databasen. Alle seks `BLOB_S3_*` eller ingen |
| TSA (RFC 3161) | Valgfritt | `ANCHOR_TSA_URL` gir tredjeparts tidsstempel på forankringen |
| OCR-sidecar | Valgfritt | `regnmed-ocr` gir tekstforslag på kvitteringsfoto |

Ressursbudsjettet er en produktdel: `regnmed-api` bruker rundt 13 MB RSS under last mot et tak på 64 MB, og håndheves i CI ([frugality](/teknisk/frugality/)).

## Image

Utgitte images ligger på `ghcr.io/bogentech/regnmed` med tagger som `v0.13.6` og `sha-<kort>`. Ett runtime-image (distroless, statiske musl-binærer) inneholder:

- `regnmed-api`, API-et som også serverer portalen (inngangspunktet)
- `regnmed`, administrasjons-CLI-et (`migrate`, `verify-ledger`, `anchor` m.fl.), brukt av migrasjons- og cron-jobber
- `regnmed-agent`, arbeideren for vertsagenter
- `regnmed-ocr` med modeller

Bygg for riktig arkitektur: imaget har bevisst ingen standard `TARGET`, siden en feil arkitektur først feiler ved containerstart. Bruk `scripts/build-images.sh`.

## Lokalt på en maskin

```sh
docker compose up -d            # Postgres 18 på port 5433 (eller scripts/dev-db.sh uten Docker)
cp .env.example .env
cargo run -p regnmed-cli -- migrate
cargo run -p regnmed-api
```

`.env.example` er den fullstendige referansen for hver miljøvariabel koden leser, med dummyverdier. Den må aldri inneholde en ekte hemmelighet.

## Variablene som betyr noe

| Variabel | Betydning |
| --- | --- |
| `DATABASE_URL` | **Påkrevd.** Postgres-tilkobling |
| `OIDC_ISSUER` | **Påkrevd.** Issuer, oppdages via `/.well-known/openid-configuration` |
| `OIDC_AUDIENCE` | Hvilken `aud` som godtas. Uten den sjekkes ikke audience |
| `OIDC_JWKS_FILE` | Kun utvikling og tester: last JWKS fra fil |
| `BIND_ADDR` | Standard `127.0.0.1:8080`, altså loopback. En container må sette den eksplisitt |
| `PORTAL_BASE_URL` | Lenken i invitasjons-e-posten. Aldri hentet fra `Host`-headeren |
| `PORTAL_OIDC_CLIENT_ID` | Portalens klient. Standard `regnmed-portal` |
| `NATS_URL`, `NATS_USER`, `NATS_PASSWORD` | Mail-rail. Uten dem er e-post av |
| `MAIL_IN_DOMAIN` | Domenet innkommende bilagsadresser vises under. Sett den først når MX faktisk leverer |
| `ANCHOR_TSA_URL` | RFC 3161-vitne for forankringen |
| `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`, `REGNMED_DRIFT_ORGNR` | Kortskinnen for abonnement. Begge Stripe-variablene eller ingen. Er du ikke en tjeneste som tar betalt, trenger du dem ikke |
| `AGENT_SEALING_KEY` | Kreves for vertsagenter (kundens LLM-nøkkel lagres forseglet) |
| `MASKINPORTEN_*` | Offentlige integrasjoner. Alle påkrevde sammen eller ingen |

I Kubernetes (`deploy/`, kustomize med overlays `local`, `test`, `prod`) kommer hemmelighetene fra Kubernetes-secrets laget utenfor git. **Ingen hemmeligheter i repoet, i noen form**, heller ikke krypterte.

## Etter første utrulling

Manifestene kan ikke opprette tilstanden inne i regnid. To steg gjøres én gang per miljø:

```sh
regnid add-client --client-id regnmed-portal --name "regnmed portal" \
    --redirect-uri https://din.host/callback \
    --post-logout-redirect-uri https://din.host/ \
    --audience regnmed
```

`--audience` er det som svir hvis det glemmes: innloggingen virker, og så svarer hvert API-kall `401`. Den første kontoen lages med `regnid add-user`. En selskapsadministrator lages ikke manuelt: den som onboarder selskapet fra Enhetsregisteret blir administrator. Fullt oppsett, backup med ukentlig gjenopprettingstest og herding: [Deployment](/teknisk/deploy/).

## Ofte brukte drifts-kommandoer

```sh
regnmed migrate               # kjør migrasjoner
regnmed verify-ledger         # gå alle kjeder fra genesis, sjekk vedlegg og forankring
regnmed anchor                # ta et forankringssnapshot (nattlig cronjob)
```

## Hva AGPL-3.0 betyr for deg

- Du kan fritt kjøre, endre og selvhoste regnmed.
- Endrer du den og lar andre bruke den **over et nettverk**, må du gjøre den komplette tilsvarende kildekoden tilgjengelig for dem (AGPL §13). Kjører du den uendret for deg selv, forplikter ingenting.
- Kunder av den hostede tjenesten trenger ingen lisens. En separat kommersiell lisens finnes etter avtale for det uvanlige tilfellet der AGPL ikke passer, men det er ikke slik regnmed selges. Produktet er tjenesten.

Se [LICENSE](https://github.com/BogenTech/regnmed/blob/main/LICENSE) og [COMMERCIAL-LICENSE.md](https://github.com/BogenTech/regnmed/blob/main/COMMERCIAL-LICENSE.md).
