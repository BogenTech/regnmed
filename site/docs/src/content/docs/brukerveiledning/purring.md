---
title: Purring og betalingsoppfølging
description: Forfalte fakturaer, purretrappen, gebyr og forsinkelsesrente, og hvor regnmed stopper.
sidebar:
  order: 4
---

regnmed regner ut hva som er forfalt, foreslår neste steg og beregner gebyr og rente etter satsene som gjelder. Du bestemmer og sender selv: ingenting purres av seg selv.

## Se hva som er forfalt

«Forfalt» er alltid beregnet fra forfallsdato og det som står igjen på kundens åpne post, aldri et lagret flagg. Kreditnotaer teller ikke.

- Under Faktura → Forfalt vises aldersfordelingen: forfalt 1–14 dager, 15–30 dager og over 30 dager, med siste purresteg per faktura.
- På Oversikt viser «Til oppfølging» antall forfalte fakturaer og beløpet.
- På selve fakturaen står neste steg fremhevet. Er fakturaen forfalt men aldri sendt, ber portalen deg sende den først, siden en purring til noen som aldri fikk fakturaen ikke hjelper.

## Purretrappen

Åpne den forfalte fakturaen og trykk «Purring». Trappen har tre steg, og den går bare én vei:

| Steg | Gebyr | Krav |
| --- | --- | --- |
| Betalingspåminnelse (gebyrfri) | Aldri | Ingen |
| Purring | Purregebyr tidligst 14 dager etter forfall | Maks to gebyrbelagte steg per krav |
| Inkassovarsel (14 dagers frist) | Som over | Minst 14 dagers betalingsfrist, og dokumentet bærer lovteksten |

Etter et strengere steg kan du ikke gå tilbake til et mildere.

I skjemaet velger du **Steg** og **Betalingsfrist**, og kan huke av for:

- **Purregebyr (maks-sats)**. Taket er satsen som gjelder på sendedatoen, hentet fra satsregisteret med kilde.
- **Krev forsinkelsesrente**. Renten løper fra dagen etter forfall, faktiske dager delt på 365, og deles opp på tvers av de halvårlige rentesatsene. Hver periode rundes for seg, så spesifikasjonen i dokumentet summerer nøyaktig til totalen.
- **Næringsdrivende skyldner (standardkompensasjon)**. Mot næringsdrivende gjelder standardkompensasjon i stedet for det vanlige gebyrtaket.

Trykk **Forhåndsvis** først. Du ser teksten og beløpet som blir registrert. Endrer du et valg etter forhåndsvisningen, forkastes den, og du må forhåndsvise på nytt før «Registrer». Etter registrering tilbys «Send til kunde» (eller «Senere»).

## Hva som bokføres

Gebyr og rente bokføres som et vanlig bilag i samme operasjon som steget registreres: debet kundefordring på kunden, kredit inntekt, uten mva. Kravet blir en ny åpen post på samme kunde, med samme KID hvis fakturaen har en, så en innbetaling fortsatt finner riktig faktura. Feiler bokføringen, registreres ikke steget. Den opprinnelige fakturaen er uberørt til den betales.

## Historikken kan ikke endres

Under **Purrehistorikk** på fakturaen ser du hvert steg med dato, frist, gebyr og rente og bilaget. Dokumentet lagres slik det ble registrert, og kan lastes ned som tekst eller PDF og sendes på e-post. Det er byte for byte det samme for alltid, uansett hvordan satsene endres senere.

## Der regnmed stopper

regnmed går ikke lenger enn inkassovarselet. Inkasso krever bevilling, og saken overlates til et inkassoselskap. Det står i selve dokumentet.

:::note
Renten er fastsatt når steget registreres. Rente som påløper etter siste steg frem til betalingsdagen beregnes ikke automatisk (en rentenota ved innbetaling er ikke bygget).
:::
