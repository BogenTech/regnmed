# Skjermbilder

Ekte skjermbilder av portalen, tatt mot en seedet demo (fiktivt selskap «Fjordvik Konsult AS», byrået «Nordlys Regnskap AS»; ingen ekte data). Dette er kilden: både `site/www` og `site/docs` kopierer mappen til `public/screens/` ved bygg (`site/scripts/sync-screens.mjs`).

Filene heter `<slug>-light.png` og `<slug>-dark.png` (tema `steelzombie`), 1440×900 ved 2×. Mobil er 390×844 ved 2× (`-mobile-light.png`).

| Slug | Viser |
| --- | --- |
| `oversikt` (+ mobil) | Dashbord: til oppfølging, utestående, resultat per måned |
| `faktura` (+ mobil) | Fakturaliste med forfalt/betalt og filtre |
| `hovedbok` | Konto 1920 med postering og løpende saldo (drill-down fra kontoplanen) |
| `bilag` | Innboks: PDF ved siden av bokføringsskjema fylt ut av bilagstolkningen («Forslag») |
| `mva` | Mva-spesifikasjon for 5. termin med frist, grunnlag og å betale |
| `bank` | Bankavstemming: poster i banken uten match |
| `revisjon` | Verifikasjonsrapporten: hash-kjede fra genesis, vedlegg, forankring, reskontro mot hovedbok |
| `lonn` | Kjør lønn + lønnskjøringer og a-melding |
| `timer` | Ukegrid for timeføring |
| `byra` | Byråvisning: klienter og medlemmer |
| `integrasjoner` | Agenter og integrasjoner (bring-your-own-agent) |

## Lyst og mørkt

Begge nettstedene viser bildet som passer temaet leseren ser. `site/www` bruker `Shot.astro` (bare `slug`). I `site/docs` skrives bildet som et par, lyst først, og `docs.css` skjuler det som ikke passer:

```html
<img src="/screens/mva-light.png" alt="…" width="1440" height="900" loading="lazy" />
<img src="/screens/mva-dark.png" alt="…" width="1440" height="900" loading="lazy" />
```

`loading="lazy"` er det som hindrer at det skjulte bildet lastes ned. Et bilde uten mørk tvilling (mobilbildene) skrives alene og vises i begge temaer.

Ikke tatt: ferdig rendret faktura-PDF som eget bilde (PDF-en vises i `bilag` i stedet).

## Lage dem på nytt

Krever Docker, Rust, Node og Google Chrome. `puppeteer-core` installeres utenfor repoet (ingen nedlasting av nettleser, Chrome brukes som den er):

```sh
mkdir -p /tmp/pptr && (cd /tmp/pptr && npm init -y && npm i puppeteer-core)

docker compose up -d
docker exec regnmed-db-1 psql -U regnmed -c "create database regnmed_shots"
export DATABASE_URL=postgres://regnmed:regnmed@localhost:5433/regnmed_shots
mkdir -p .dev/shots
cargo run -p regnmed-cli -- migrate
SEED_BROWSER=$PWD/.dev/shots cargo test -p regnmed-api --test seed_browser seed_browser_demo   # roterer demo-tokenet

OIDC_ISSUER=https://id.test.invalid OIDC_AUDIENCE=regnmed OIDC_JWKS_FILE=$PWD/.dev/shots/jwks.json \
  BIND_ADDR=127.0.0.1:8099 cargo run -p regnmed-api &

# Beriker demoen via det offentlige API-et (kjøres ÉN gang per tom database):
node site/scripts/seed-demo.mjs http://127.0.0.1:8099 $PWD/.dev/shots docker exec regnmed-db-1 psql -U regnmed -d regnmed_shots
docker exec regnmed-db-1 psql -U regnmed -d regnmed_shots -At -c "select id from firm limit 1" > .dev/shots/firm.txt

FIRM_ID=$(cat .dev/shots/firm.txt) PPTR=/tmp/pptr \
  node site/scripts/capture-screens.mjs http://127.0.0.1:8099 $PWD/.dev/shots site/screens   # valgfritt 4. arg: slug,slug

# Rydd: stopp API-et, slett databasen og tokenene
docker exec regnmed-db-1 psql -U regnmed -c "drop database regnmed_shots with (force)"
rm -rf .dev/shots
```

Seed-identiteten kommer fra tokenet («Demo Bruker», en `test.invalid`-adresse). Capture-skriptet skriver om JSON-svar fra API-et underveis til «Ingrid Solberg», slik at ingenting av testidentiteten havner i bildene. Banken viser «ikke oppgitt» som banksaldo fordi demo-kontoutskriften ikke har saldolinjer.
