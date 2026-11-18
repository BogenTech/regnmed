---
title: Valuta
description: Fakturere i utenlandsk valuta, hente kurser fra Norges Bank og hvordan kursforskjeller bokføres.
sidebar:
  order: 34
---

Hovedboken føres alltid i **norske kroner**. Dokumenter bærer valutaen de lød på, og posteringene lagrer både valutabeløpet og kursen som ble brukt, som bevis. Kursen er en del av den kjedede historikken, så den kan ikke omskrives i ettertid.

## Kurser

Under Bank → **Valutakurser** (kortet «Valutakurser»):

- **Hent fra Norges Bank** henter kurser fra Norges Banks åpne API.
- **Legg inn kurs for hånd** («Lagre kurs») legger inn en kurs selv. Den som legger den inn står som kilde.

Hver kurs har dato og kilde, og kurser slettes ikke. Regnmed bruker siste notering på eller før datoen (Norges Bank noterer bare på bankdager). Er det ingen kurs for datoen eller før, feiler det med en tydelig melding i stedet for å gjette.

## Faktura i valuta

Velg valuta når du lager fakturaen. Linjebeløpene er i valutaen, og kursen på **fakturadatoen** hentes fra registeret. Hver linje regnes om til kroner, og kundefordringen er den nøyaktige summen av delene. PDF-en viser beløpene i valutaen og motverdien i kroner.

En kreditnota reverseres til **opprinnelig kurs**, så kronebeløpet nulles ut.

Regnmed sjekker at kroneverdien stemmer med beløp ganger kurs innenfor 1 kr. Større avvik er nesten alltid en enhetsfeil (for eksempel kursen forskjøvet ett siffer), og avvises.

## Kursforskjeller ved betaling

Når en betaling avstemmes mot en faktura i valuta, bokfører regnmed realisert kursgevinst eller kurstap (agio) og en overføringspostering på kunden i **samme transaksjon** som avstemmingen. Begge kronerestene går til null.

## Urealisert kursregulering

For å regulere åpne poster til dagskurs ved periodeslutt bokfører regnmed et regulerende bilag og et reverseringsbilag i én transaksjon. Kjøringen er ikke idempotent. Kjører du den to ganger, får du to regulerende bilag, så kontroller datoen først.

## Begrensninger

- Egne valutakontoer i bank og sikringsbokføring er ikke bygget. Banken føres i norske kroner.
- Timer faktureres bare i norske kroner.

Se også [Teknisk dokumentasjon: Flervaluta](/teknisk/valuta/).
