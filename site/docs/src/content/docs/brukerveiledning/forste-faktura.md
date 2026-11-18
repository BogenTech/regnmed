---
title: Første faktura
description: Fra tom portal til en utstedt, sendt faktura, steg for steg.
sidebar:
  order: 2
---

Denne veiledningen følger en faktura fra start til slutt. Mer om alt rundt (kreditnota, repeterende faktura, tilbud) står under [Fakturering](/brukerveiledning/fakturering/).

## Før du starter

Du trenger tre ting på plass:

- **Firmaopplysninger** under Administrasjon: adresse og kontonummer. Se [Kom i gang](/brukerveiledning/kom-i-gang/).
- **En kunde** med adresse. Både din og kundens adresse er påkrevd på en faktura (bokføringsforskriften §5-1-2). Utstedelsen nektes uten dem, fordi en utstedt faktura ikke kan endres. Kunden legger du inn under Kunder, eller direkte fra fakturaskjemaet med «Ny kunde?».
- **Mva-registrering** hvis du skal ta mva. Uregistrerte selskaper fakturerer uten mva-kode.

## Lag fakturaen

1. Åpne Faktura og velg «Ny faktura», eller bruk snarveien på Oversikt.
2. Velg **Kunde**. Du kan søke på navn eller kundenummer.
3. Sjekk datoene:
   - **Fakturadato**
   - **Forfall**, som er fakturadato pluss 14 dager til du endrer det
   - **Leveringsdato**, som er fakturadato til du endrer den. Fakturerer du i ettertid, sett siste leveringsdag. Loven krever at leveringstidspunktet står på fakturaen, og det er ikke alltid samme dag som fakturadatoen.
   - **Leveringssted** er valgfritt.
4. Fyll inn **Deres referanse** hvis kunden har oppgitt en (ordrenummer, avdeling, kontaktperson). Offentlige og store kjøpere ruter innkommende faktura på den. Maks 70 tegn. Står feltet tomt, brukes kundens faste referanse hvis den finnes.
5. La **Valuta** stå på NOK, eller velg en annen. Se [Fakturering](/brukerveiledning/fakturering/#faktura-i-valuta).
6. Legg inn **Linjer**. Hver linje er enten et **Produkt** fra produktregisteret eller **Fritekst**, med Beskrivelse, Antall, Pris (kr) og Mva. Summen oppdateres mens du skriver. «+ Legg til linje» gir en ny linje.
7. Trykk «Utsted faktura».

Portalen viser en oppsummering først: kunde, antall linjer, beløp og forfall. Bekreft med «Utsted faktura» først når du har sett over den. Etterpå kan fakturaen ikke endres. Feil rettes med kreditnota.

:::note
Summen i skjemaet bruker dagens satser. Serveren regner på nytt med satsen som gjaldt på fakturadatoen når du utsteder, og det er serverens tall som står på fakturaen.
:::

## Send den

Etter utstedelsen lander du på fakturaen, og «Send til kunden» er neste steg.

1. Trykk «Send til kunde».
2. Mottakeren er kundens lagrede e-postadresse. Du kan skrive en annen for denne ene utsendelsen.
3. Bekreft. Kunden får PDF-en som vedlegg, og svar går til selskapets egen e-postadresse.

Utsendelsen er alltid en handling du gjør selv. regnmed sender aldri en faktura av seg selv. Har du ikke e-post å sende til, bruker du «Last ned PDF» og sender den på den måten du vil.

![Fakturalisten med status per faktura](/screens/faktura-light.png)

## Hva er skjedd

Utstedelsen gjorde fire ting i én og samme operasjon:

1. Fakturaen fikk neste ledige fakturanummer. Nummerserien har aldri hull.
2. Den ble bokført: debet kundefordring (1500) på kunden, kredit inntekt per linje, kredit mva.
3. PDF-en ble lagret som vedlegg til bilaget.
4. Hvis du har KID-avtale, fikk fakturaen en KID som betalingen kan merkes med.

Feiler noe, skjer ingenting av det, og nummeret er ikke brukt.

## Når kunden betaler

- **Med bankavstemming:** importer kontoutskriften under Bank, så kobles innbetalingen til fakturaen. Se [Bank](/brukerveiledning/bank/).
- **Utenom banken** (kontant, Vipps, en enkeltinnbetaling): åpne fakturaen og trykk «Registrer innbetaling». Bilaget bokføres og kobles mot fakturaen i samme operasjon. Fakturasiden viser etterpå hver innbetaling med bilaget den er bokført på.

Status på fakturalisten er Ubetalt, Forfalt, Betalt eller Kreditnota, og beregnes alltid fra reskontroen.
