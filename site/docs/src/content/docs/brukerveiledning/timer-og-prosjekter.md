---
title: Timer og prosjekter
description: Føre timer i ukegridet, sette satser på prosjektet, fakturere timer og se om et prosjekt lønner seg.
sidebar:
  order: 31
---

## Ukegridet

**Timer** åpner med et regneark: radene er prosjekter, kolonnene er dagene i uken, og hver celle er timene den dagen. Overskriften sier hvilken uke du er i (for eksempel «Uke 46 · 9.–15. november 2026»), med «Forrige uke», «Denne uken» og «Neste uke».

- Skriv timer som `7,5`, `7.5` eller `7:30`. Cellen lagrer seg når du forlater den.
- Enter og piltaster flytter mellom cellene.
- Nye rader arver prosjektene fra forrige uke. Du kan også kopiere forrige uke.
- Låste dager er deaktivert. Fakturerte celler viser fakturaen.
- På mobil vises én dag av gangen.

Timene lagres som hele minutter, aldri som flyttall.

## Prosjekter

Under **Prosjekter** ligger registeret: kode, navn, kunde, og om timene som standard er fakturerbare. Prosjektkoden er permanent fordi den inngår i regnskapet. Navnet kan endres. Et prosjekt slettes aldri, bare avsluttes, og et avsluttet prosjekt nekter nye føringer.

Et prosjekt kan knyttes til én kunde. Kunden brukes som forslag når du fakturerer, aldri som automatikk.

## Satser

Prosjektet eier satsen. Du kan sette en standardsats og en egen sats per person. Satsene har gyldighetsdato, og en føring prises på sin egen dato: personens sats først, deretter prosjektets standard.

- Uten rettigheten til å sette sats gjelder alltid satsen fra prosjektet, uansett hva som sendes inn.
- Fakturerbare timer uten sats **nektes** med en melding. En stille 0 ville havnet på en faktura.
- Timer som er ført beholder satsen de ble ført med. De prises aldri på nytt.

## Hvem ser hva

En ansatt ser bare sine egne timer. De som kan se alles timer (bokføring, revisor, admin) ser hele laget. Se [Tilgang og roller](/brukerveiledning/tilgang-og-roller/).

## Låse måneden

Når en måned er godkjent, kan du låse den. Låste timer kan ikke endres, og det håndheves i databasen. Du kan låse måneden og deretter fakturere, så godkjenningen kommer før faktureringen. Å åpne en låst måned igjen er en registrert handling.

## Fakturere timer

Fakturering av timer ligger under **Faktura**: kortet med ufakturerte timer grupperer fakturerbare timer per prosjekt og sats, med oversikt over hvem som har ført dem.

1. Velg timene du vil fakturere («Fakturer valgte»), eller ta dem med fra «Ny faktura» via «Ta med ufakturerte timer».
2. Velg kunde. Prosjektets kunde foreslås, men du velger selv.
3. Fakturaen utstedes på vanlig måte, og timene merkes som fakturert i samme steg.

En faktura kan ha både varelinjer og timelinjer. Timer faktureres bare i norske kroner. Er utvalget utdatert når du bekrefter, feiler hele fakturaen i stedet for å fakturere mindre enn du valgte.

Prosjektdimensjonen følger med inntekten, så rapportene kan vise resultat per prosjekt.

## Prosjektlønnsomhet

Under Rapporter → **Prosjekt** ser du ett prosjekt per rad: inntekt, kostnad og timer, med utsnittet fra resultatregnskapet for akkurat det prosjektet. Klikk en rad for detaljene. Rapporten er en ren sammenstilling av tall som allerede ligger i regnskapet, uten egen lagret tilstand. Poster uten prosjektkode regnes ikke som prosjektøkonomi.

<img src="/screens/timer-light.png" alt="Ukegrid for timer" width="1440" height="900" loading="lazy" />
<img src="/screens/timer-dark.png" alt="Ukegrid for timer" width="1440" height="900" loading="lazy" />
