---
title: Bidra
description: Slik bidrar du til regnmed. CLA, bidragsytertilbud, test- og språkregler.
sidebar:
  order: 11
---

Bidrag er velkomne. Koden ligger på [github.com/BogenTech/regnmed](https://github.com/BogenTech/regnmed).

## CLA

regnmed er AGPL-3.0, og vedlikeholderen beholder muligheten til å lisensiere på andre vilkår. For å holde det mulig må **alle bidragsytere godta [CLA-en](https://github.com/BogenTech/regnmed/blob/main/CLA.md)** før bidraget kan flettes inn. Kort fortalt beholder du opphavsretten, og du gir vedlikeholderen en bred lisens til å bruke, endre og lisensiere bidraget på nytt. Du bekrefter også at det er ditt eget arbeid.

Legg denne linjen i beskrivelsen av din første pull request:

> I have read the CLA and agree to its terms.

## Bidragsytertilbud

CLA-lisensen er lik for alle. Det som forhandles per bidrag, etter omfang og verdi, er **ditt eget abonnement på den hostede tjenesten**: pris (også gratis), plan og varighet. Ta kontakt med vedlikeholderen før eller etter. Selvhosting er allerede gratis under AGPL, så tilbudet gjelder tjenesten vi faktisk tar betalt for.

## Slik går det til

1. Fork og lag en funksjonsgren.
2. Gjør endringene i små, tydelige commits.
3. Åpne en pull request som sier hva og hvorfor.
4. Ta med CLA-linjen i den første.

Hver endring som går inn i `main` gjennomgås før den flettes. Hver funn rettes i samme PR eller navngis som bevisst utsatt, med grunn. Grønn CI er ikke en gjennomgang.

## Bygg og test

```sh
docker compose up -d                 # Postgres 18 på port 5433, eller scripts/dev-db.sh
cp .env.example .env
cargo run -p regnmed-cli -- migrate
cargo test                           # enhetstester, ingen database nødvendig
cargo nextest run --workspace        # hele suiten (trenger DATABASE_URL), ca. 3,6 ganger raskere
cargo fmt --all                      # CI feiler på cargo fmt --all --check
```

Integrasjonstester hopper høflig over når `DATABASE_URL` mangler, og kjører for ekte i CI mot Postgres 18. API-integrasjonstestene er **gruppert** i `crates/regnmed-api/tests/grupper/`: ny test hører i en eksisterende gruppe, ikke i en ny fil i `tests/`.

Hva som krever test i samme commit: domeneinvarianter (penger, hashing, dobbelt bokholderi), uforanderlighet i hovedboken, tilgangsgrenser og eksportformater. En autorisasjonstest som ikke er sett feile er ikke verifisert.

:::caution[Den frosne hashen]
`golden_hashes_never_change` i `regnmed-core::hash` fester nøyaktig digest av den kanoniske serialiseringen. Feiler den, bryter endringen verifiseringen av hver utrullet hovedbok. Formatet kan bare versjoneres, aldri redigeres. Det samme gjelder forankringsformatet.
:::

## Arkitekturregler du må kjenne

- **Penger er heltall øre** (`Ore(i64)`), aldri flyttall. Saldo er alltid `SUM(amount_ore)`, aldri lagret tilstand.
- **Hovedboken er append-only**, håndhevet i domene, database og kryptografi.
- **Migrasjoner er append-only.** Rediger aldri en anvendt migrasjon.
- **`regnmed-core` har ingen I/O eller databaseavhengigheter**, noensinne.
- **Autorisasjon ligger i regnmeds database, ikke i tokens.** IdP-spesifikke detaljer bakes aldri inn.
- **API-først.** Alt en bruker kan gjøre er et autentisert endepunkt. CLI-et er bare for drift.

## Språkregler

**Alt internt er engelsk. Norsk bare der en bruker leser det.**

| Hvor | Språk |
| --- | --- |
| Identifikatorer, databasetabeller og kolonner, CLI-kommandoer, kommentarer, testnavn, commit-meldinger | Engelsk |
| Logger og feilmeldinger en operatør leser | Engelsk |
| Tekster en bruker leser (portalen, feilmeldinger i UI-et), prompter til en modell | Norsk |
| `docs/` (revisjonsvendt) | Norsk |

Unntak: **anvendte migrasjoner er append-only**, så en eksisterende kolonne beholder navnet sitt, og du mapper den til et engelsk felt i koden. **Nettformatet står fast**: JSON-nøkler (`orgnr`, `termin`, `prosjekt`) er festet med `#[serde(rename)]` og endres ikke ved en refaktorering, siden portalen og kundenes integrasjoner leser dem. CLI-kommandonavn er også festet av en test (`the_ops_interface_has_not_moved`).

Skal du omdøpe en identifikator mekanisk, finnes verktøyene som verifiserer at strenger, nettnavn og CLI-flater er uendret (`scripts/rename-identifiers.py`, `scripts/wire-names.py`, `scripts/pin-wire.py`).

## Ikke gjør

- Ikke legg hemmeligheter i repoet, i noen form. Se [Selvhosting](/utviklere/selvhosting/).
- Ikke rediger en anvendt migrasjon.
- Ikke legg en ny `tests/*.rs`-fil for en enkelt test.
