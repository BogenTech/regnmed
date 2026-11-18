---
title: MVA
description: Mva-koder, terminer, terminordning, mva-spesifikasjonen og mva-meldingen, med hva som kan sendes fra regnmed i dag.
sidebar:
  order: 7
---

Mva-seksjonen viser avgiften slik den står i hovedboken, per termin og per kode, og lager mva-meldingen. Du leverer meldingen selv. Se «Mva-meldingen» lenger ned for hva som er og ikke er bygget.

## Mva-koder

regnmed bruker Skatteetatens standard mva-koder direkte. Det er de samme kodene som står i SAF-T og i mva-meldingen, så det finnes ingen oversettelse som kan gå galt. Du velger kode på hver linje i fakturaskjemaet, i bokføringsskjemaet og i Nytt bilag. Satsen hentes fra en datert tabell, så et bilag får alltid satsen som gjaldt på bilagsdatoen. Skifter satsen, ser et gammelt bilag likevel ut som da det ble bokført.

Registreringen avgjør om en kode kan brukes i det hele tatt:

- Utgående mva kan bare kreves inn av en mva-registrert selger, og inngående mva bare fradragsføres av en registrert kjøper. Statusen ligger i Firmaopplysninger under Administrasjon, med dato.
- Uregistrerte selskaper kan bare bruke kodene 0, 6 og 7, som sier at transaksjonen ligger utenfor mva-systemet. Kode 5 (fritatt omsetning) krever registrering.
- Vakten sitter i bokføringsveien, ikke bare i skjemaene. Den gjelder faktura, utlegg, innboks, manuelle bilag og kassaoppgjør. Importert historikk er unntatt.

Gjelder koden kjøp fra utlandet med omvendt avgiftsplikt, bokfører regnmed den beregnede avgiften selv (kontoene 2701 og 2711), slik at den står i regnskapet og ikke bare i rapporten.

## Terminer og terminordning

En termin er normalt to måneder: 1 er januar–februar, og 6 er november–desember. Fristen for å levere er en måned og 10 dager etter terminen, med særregelen 31. august for 3. termin.

Noen selskaper har en annen ordning, etter vedtak fra Skatteetaten:

- **Årstermin** (frist 10. mars) for virksomhet under omsetningsgrensen, etter søknad.
- **Primærnæring** (frist 10. april).

Under «Terminordning» på Mva-siden registrerer du **Ny ordning** med **Gjelder fra** og en **Vedtaksreferanse** (for eksempel Skatteetatens brev). Ordningen som Skatteetaten har gitt deg registreres, den gjettes aldri ut fra tall. Historikken ligger igjen, og spesifikasjonen, meldingen og terminvelgeren følger ordningen. Terminer utenfor ordningen avvises. Kortere terminer som pålegges ved restanse er ikke bygget.

## Mva-spesifikasjonen

Velg år og termin på Mva-siden. Du får en tabell per kode med **Grunnlag**, **Mva**, **Utgående mva** og **Fradrag**, og summen å betale eller til gode. Åpne en kode for å se posteringene bak: bilag, dato, konto, tekst, part og grunnlag.

Avgiften er **beregnet** fra grunnlaget per sats, ikke summen av det som er bokført. Det er med vilje: forskjellen mellom spesifikasjonen og saldoen på mva-kontoene (2700 og 2710) er nettopp kontrollen regnskapsføreren skal gjøre før meldingen sendes. En termin som går over et satsskifte har en linje per sats.

## Mva-meldingen

Mva-meldingen lages av de samme linjene som spesifikasjonen, i hele kroner og med fortegn slik Skatteetaten leser dem. Kode 0 rapporteres ikke. Filen kontrolleres mot Skatteetatens offisielle skjema.

Under «Mva-meldingen» trykker du **«Last ned mva-meldingen (XML)»**.

:::caution
Direkte innsending fra regnmed er ikke på plass ennå. Skatteetaten har gitt regnmed tilgang til valideringstjenesten i testmiljøet, men innsendingen går gjennom Altinn og er ikke tatt i bruk. Til da laster du ned meldingen for å kontrollere den og leverer gjennom et system som kan sende den.
:::

Det er også et åpent spørsmål, som avklares mot valideringstjenesten før første innsending, om tosidige koder (omvendt avgiftsplikt) skal sendes som én eller to linjer. Det endrer ikke beløpet som skal betales, bare linjeformen.

SAF-T for hele året ligger under Rapporter.

## Kontroller før du leverer

1. Åpne spesifikasjonen for terminen og se på hver kode. Åpne posteringene bak en kode du ikke kjenner igjen.
2. Sammenlign netto med saldoen på mva-kontoene.
3. Er alle bilag bokført, og er perioden klar til å låses? Se under Administrasjon → Periode.
4. Last ned meldingen og lever.

![Mva-spesifikasjon per kode](/screens/mva-light.png)
