---
title: Bank
description: Importer kontoutskrifter, avstem banken mot regnskapet og lag betalingslister som lastes opp i nettbanken.
sidebar:
  order: 8
---

Bank-seksjonen viser at kontoen i regnskapet stemmer med bankens egne tall, og lar deg betale leverandører med en fil fra regnmed.

Seksjonen har fanene **Avstemming**, **Betalinger**, **Valutakurser** og **Firmakort**.

<img src="/screens/bank-light.png" alt="Bankavstemming med foreslåtte koblinger" width="1440" height="900" loading="lazy" />
<img src="/screens/bank-dark.png" alt="Bankavstemming med foreslåtte koblinger" width="1440" height="900" loading="lazy" />

## Slik kommer kontoutskriften inn

regnmed har ikke direkte forbindelse til banken. Du laster ned kontoutskriften fra nettbanken og laster den opp:

- **camt.053** (XML), som alle norske nettbanker kan eksportere. Den har inngående og utgående saldo.
- **CSV** fra nettbanken. regnmed leser overskriftene for å finne kolonnene (dato, beløp eller inn/ut, KID og referanse) og kan lese norsk tallformat. Skjønner den ikke filen, stopper den og sier hvilke overskrifter den så, i stedet for å importere halvparten.

Trykk «Last opp kontoutskrift» eller dra filen inn. Hver kontoutskrift lagres uforanderlig, og samme utskrift importeres bare én gang. Bankkontoen velger du øverst («Bankkonto»), og selskapet må ha en bankkonto (19xx) i kontoplanen. Mangler den, legger du den til under Hovedbok.

Direkte kontofeed fra banken (PSD2) og filutveksling med bank er ikke bygget.

## Avstemming

Toppen av siden viser status: «Alt i banken er avstemt», eller «N bankposter å avstemme», med **Banken** (utgående saldo), **Hovedboken samme dato** og **Differanse**. CSV-filer har ingen saldo, så differansen kan ikke regnes fra dem, og den vises som et tankestrek. Saldoen er fraværende, ikke null.

regnmed kobler selv de postene som er entydige: samme beløp og samme dag først, så samme beløp innenfor fem dager. Hver bankpost og hvert bilag brukes bare én gang. **Er det flere like gode kandidater, gjetter regnmed ikke**: posten blir liggende for manuell behandling.

Under «I banken, ikke i regnskapet» står hver ukoblede bankpost, og du har tre veier:

1. **Koble til bilag …** er forslag med akkurat samme beløp. Opptil tre vises, nærmeste dato først. Et annet beløp foreslås aldri, og en manuell kobling av ulike beløp avvises med beløpene navngitt.
2. **Bokfør** for poster som ikke har noe bilag bak seg, som gebyr og renter. Velg konto, justér «Tekst på bilaget» og trykk «Bokfør og koble». Bilaget og koblingen lages i én operasjon, uten mva. Har utgiften mva eller kvittering, hører den hjemme i innboksen. Med aktiv attestering over grensen må den også gå der.
3. **Finn bilag** søker på beløp, tekst eller bilagsnummer og lar deg «Koble».

Under «I regnskapet, ikke i banken ennå» ligger bilag som er bokført på bankkontoen men ikke har en bankpost, ofte betalinger som ikke er trukket ennå. Under «Koblet» kan du «Fjern kobling» hvis en kobling var feil. «Ikke avstemt» er alltid fraværet av en kobling, aldri en lagret tilstand.

Kontoutskriften fra et firmakort importeres på samme måte, på kortets egen konto. Kort registrerer du under fanen Firmakort.

## Betalingsliste (pain.001)

Fanen **Betalinger** gjør åpne leverandørposter og godkjente utlegg om til en fil du laster opp i nettbanken. regnmed sender ikke betalingen selv.

1. Under «Å betale» ser du åpne leverandørposter og godkjente utlegg med forfall og KID. «Velg forfalte» eller «Velg alle» hjelper deg å velge. Velg **Betalingsdato**.
2. Trykk «Lag betalingsliste». Posten må ha gyldig kontonummer. Mangler leverandøren det, står det «mangler konto» med en lenke for å legge det inn under Leverandører. For ansatte legges kontonummeret inn under Lønn.
3. Listen vises under «Betalingslister i arbeid». **«Godkjenn»** fryser listen og lager filen, som lagres med kontrollsum. Å lage og å godkjenne er to separate handlinger. Med aktiv attestering må godkjenneren være en annen enn den som lagde listen.
4. **«Last ned betalingsfil»** og last den opp i nettbanken. Filen er en ISO 20022 pain.001-fil, med KID som referanse der den finnes.
5. Når banken har betalt, trykker du **«Registrer betalt»**. Det bokfører ett utbetalingsbilag og lukker hver leverandørpost i samme operasjon. Når du senere importerer kontoutskriften, kobles bankposten mot det bilaget.

En liste i utkast kan **annulleres**, men en godkjent liste kan ikke det: filen finnes, og hva som skjedde i banken er et oppgjørsspørsmål. Radene er en kopi av mottakerens opplysninger fra den dagen listen ble laget, så filen kan gjenskapes for alltid.

Støtten gjelder innenlandske betalinger i NOK til norske kontonumre. Utland med IBAN og BIC er ikke bygget. Kjøregodtgjørelse betales med lønnen, ikke her.

## Valutakurser

Under **Valutakurser** kan du «Hente fra Norges Bank» eller «Lagre kurs» for hånd. Kursene er daterte og beholdes, og en faktura i valuta bruker kursen på dagen. Se [Fakturering](/brukerveiledning/fakturering/#faktura-i-valuta).
