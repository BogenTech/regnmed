---
title: Introduksjon
description: Slik er regnmed-API-et bygget, hva du kan stole på, og hvor du begynner som utvikler.
sidebar:
  order: 1
---

**Nettet er produktet, og API-et er plattformen.** Alt portalen gjør, gjør den over det samme API-et du kaller selv. Et menneske og en integrasjon treffer de samme endepunktene med de samme reglene, og det finnes ingen bakdør for noen av dem. Kommandolinjen (`regnmed`) er for drift og administrasjon, og er aldri eneste vei til en funksjon.

## Adresser

| Hva | Adresse |
| --- | --- |
| API | `https://api.regnmed.no` |
| Identitetstilbyder (regnid) | `https://id.regnmed.no` |
| Portal | `https://regnmed.no` |
| Åpen forankringsfeed | `https://api.regnmed.no/anchors` |

Portalen og API-et serveres av samme binær, så `https://regnmed.no` svarer også på API-stiene. Bruk `api.regnmed.no` i integrasjoner.

## Modellen: person, firma, oppdrag, selskap

```text
person ──medlem av──▶ firma (regnskapsfører / revisor)
   │                      │
   │                      └── oppdrag (omfang + gyldighet) ──▶ selskap
   └──direkte medlem (rolle)────────────────────────────────▶ selskap
```

- En **person** er en identitet fra regnid. Et menneske og en robot er begge personer.
- Et **selskap** eier en hovedbok. All tilgang går til et selskap.
- Et **firma** er et regnskaps- eller revisjonsbyrå. Det får tilgang til selskaper gjennom **oppdrag** (avtale med omfang og gyldighetsperiode), ikke gjennom en liste i tokenet.
- Tilgangen din er **union av alle veier inn**: direkte medlemskap med en rolle, oppdrag via et firma, og for en maskin et tildelt rollenavn.

Hovedregelen er: **tokenet beviser identitet, regnmed avgjør hva identiteten får gjøre.** Tilgangen ligger i regnmeds database, aldri i tokenet. Se [Tilgang og roller](/developers/access-and-roles/).

## Hva du kan stole på

- **Beløp er heltall øre**, aldri flyttall. Positivt er debet, negativt er kredit.
- **Hovedboken er append-only.** Rettelser er reverserende bilag. Du kan verifisere kjeden selv, se [Verifiser selv](/developers/verify-it-yourself/).
- **Ingen tilgang gir `404`, ikke `403`.** Du lærer ikke at et selskap finnes uten å ha tilgang til det.

## Stabilitet

Endepunktene i [API-referansen](/developers/api/) brukes av portalen og regnes som stabile. Felter kan komme til. Eksisterende felter fjernes eller endrer betydning bare med et varsel i CHANGELOG og en overgangsperiode. Nye endepunkter dukker opp med hver sak i ROADMAP. Responsene har ingen versjonsprefiks i dag.

JSON-feltene er delvis norske, fordi nettformatet er eldre enn språkpolicyen (`orgnr`, `termin`, `leveringsdato`, `prosjekt`). Eksemplene i denne dokumentasjonen bruker nøklene akkurat slik de står på tråden.

## Hvor begynner du?

| Du vil … | Les |
| --- | --- |
| Kalle API-et fra et script eller en nettbutikk | [Kom i gang](/developers/getting-started/) |
| Forstå token, `aud` og klientregistrering | [Autentisering](/developers/authentication/) |
| Gi en robot riktig tilgang | [Tilgang og roller](/developers/access-and-roles/) |
| Se hvordan beløp, datoer og feil ser ut | [Konvensjoner](/developers/conventions/) |
| Kopiere ferdige kall | [Eksempler](/developers/examples/) |
| Slå opp et endepunkt | [API-referanse](/developers/api/) |
| Gi en LLM-agent verktøy | [MCP og agenter](/developers/mcp-and-agents/) |
| Kontrollere at hovedboken ikke er rørt | [Verifiser selv](/developers/verify-it-yourself/) |
| Kjøre regnmed selv | [Selvhosting](/developers/self-hosting/) |
| Bidra til koden | [Bidra](/developers/contributing/) |

Revisjonsvendt dokumentasjon om hva systemet garanterer og hvor det testes, ligger under [Teknisk dokumentasjon](/teknisk/).
