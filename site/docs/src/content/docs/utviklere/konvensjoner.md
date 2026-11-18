---
title: Konvensjoner
description: Beløp, datoer, feil, ratebegrensning, idempotens og paginering. Reglene som gjelder på tvers av alle endepunkter.
sidebar:
  order: 5
---

## Beløp: heltall øre

Alle beløpsfelter som ender på `_ore` er **heltall i øre**. 1 250,00 kr er `125000`. Det finnes ingen flyttall i API-et. **Positivt er debet, negativt er kredit**, også i svarene. Beløp i fremmed valuta bæres i valutaens minste enhet (cent), og et bilag er alltid autoritativt i NOK.

Mengder angis i tusendeler: 2,5 stk er `"quantity_milli": 2500`. Timer føres i heltallsminutter.

## Datoer

Datoer er `YYYY-MM-DD`. Tidspunkter er RFC 3339. Et regnskapsår er et kalenderår, og et bilag i en låst periode avvises.

## Feil

En feil har alltid formen

```json
{ "error": "tekst ment for et menneske" }
```

| Status | Når |
| --- | --- |
| `400` | Ugyldig innhold eller brutt forretningsregel. Meldingen sier hva som er galt og ofte hva du gjør |
| `401` | Tokenet mangler eller er ugyldig |
| `403` | Du har tilgang, men rollen mangler rettigheten (feilteksten er rettighetens navn), eller abonnementet er utløpt og kallet endrer noe |
| `404` | Finnes ikke, **eller** ingen tilgang |
| `429` | Ratebegrensning for integrasjoner |
| `500` | `{"error": "internal error"}`. Detaljene står bare i serverloggen |

Meldinger er på norsk der de er ment for sluttbrukeren. Les `error` som prosa. Ikke match mot teksten i kode, bortsett fra rettighetsnavnet i `403`. En ugyldig JSON-kropp eller et ugyldig spørringsfelt avvises av rammeverket før forretningslogikken, med `400` eller `422`.

## Ratebegrensning

Integrasjoner har en grense per minutt: standard **120**, justerbar per integrasjon. Over grensen svarer API-et `429` med `{"error": "for mange kall — vent litt"}`. Innlogget bruk fra mennesker begrenses ikke.

Grensen er en token-bucket **per prosess** og fylles kontinuerlig, med en kapasitet på ett minutts budsjett. Med flere replikaer er den effektive grensen per replika. Det er en bevisst avveining, ikke en forglemmelse (se [frugality](/teknisk/frugality/)). Bygg klienten med ventetid og nytt forsøk ved `429`.

## Idempotens

Det finnes ingen generell `Idempotency-Key`. Der gjentatte kall er en reell fare, har endepunktet en egen mekanisme:

- **Opplasting av bilag**: send `?sha256=<hex>` med filens SHA-256. Serveren avviser (a) innhold den allerede har, så et nytt forsøk etter et tapt svar ikke blir to bilag, og (b) en hash som ikke stemmer med bytene. Svaret har `existing: true` når innholdet var kjent fra før.
- **Fakturanumre** er hullfrie og utstedes i samme transaksjon som bilaget. En faktura er uforanderlig når den er utstedt. Rettelse er kreditnota og ny faktura.
- **Bankutskrifter** identifiseres av innholdshash, så å importere samme fil to ganger er en no-op.
- **SAF-T-import** logger innholdshash, og samme bytes kan aldri importeres to ganger.
- **Utsendelser** bruker utsendelsens id som `Nats-Msg-Id`, så meldingen aldri leveres dobbelt.

For alt annet gjelder: les tilstanden før du sender på nytt etter et tapt svar.

## Paginering

Det finnes **ingen generell paginering** i dag, og mange listeendepunkter henter alt. Enkelte har `limit`:

| Endepunkt | Parameter |
| --- | --- |
| `GET …/inbox` | `limit` (1 til 200, nyeste først), `status` |
| `GET …/parties` | `limit`, `q`, `sort` |
| `GET …/anchors` | `limit` |
| Kontospesifikasjon og bokføringsspesifikasjon | `limit` (uten `limit` kommer hele perioden) |

Dette er et åpent punkt og ikke en designavgjørelse. Begrens spørringer med `from` og `to`.

## Nedlastinger og formater

Noen endepunkter svarer med filer: `…/invoices/{id}/pdf` (PDF), `…/invoices/{id}/ehf` (UBL/EHF-XML), `…/reports/saft` (SAF-T-XML), `…/reports/revisjon?format=tekst` (ren tekst) og `…/payments/runs/{id}/file` (pain.001). Rapportene kontospesifikasjon m.fl. kan gi CSV. Dokumenter leses med hash-kontroll, så en skadet fil gir feil i stedet for stille feil innhold.
