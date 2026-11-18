---
title: Fakturering
description: Kundereskontro, kreditnota, repeterende faktura, tilbud og ordre, PDF og EHF, e-postutsendelse, KID og faktura i valuta.
sidebar:
  order: 3
---

Her er alt rundt fakturaen. Fremgangsmåten for selve utstedelsen står i [Første faktura](/brukerveiledning/forste-faktura/).

Faktura-seksjonen har fire faner: **Fakturaer**, **Tilbud og ordre**, **Repeterende** og **Timer**.

## Fakturalisten og kundereskontroen

Fakturaer-fanen viser nyeste først, med filtrene Alle, Ubetalt, Forfalt, Betalt og Kreditnota. Du kan søke på fakturanummer eller kundenavn. Å velge en rad åpner fakturaen, der alle handlingene finnes, og knappen som er det naturlige neste steget er fremhevet: send en usendt faktura, purr en forfalt.

Kundereskontroen er det som ligger bak statusen. Hver faktura er en åpen post på kunden i hovedboken. Posten lukkes av innbetalinger, kreditnotaer eller manuell kobling, og det som gjenstår beregnes alltid fra postene, aldri lagres som et tall. Under **Kunder** ser du per kunde hva som står ubetalt og hva som er forfalt, og under **Reskontro** finner du parter, import av kontakter og åpne poster, og hvilke kontoer som er reskontrokontoer.

Fakturaen viser også **Utestående**, **Sendt**, hvem som **laget** den og, når den finnes, hvilken kreditnota som krediterer den.

## Kreditnota

En utstedt faktura kan ikke endres. Er den feil, trykker du «Lag kreditnota» på fakturaen.

- Kreditnotaen er hele fakturaen med omvendt fortegn, og kobles til originalen.
- Kreditnotaen leveringsdateres med originalens leveringsdato, siden den krediterer akkurat den leveringen.
- Er fakturaen ubetalt, kobles de to postene mot hverandre automatisk, og beløpet er ute av reskontroen.
- En faktura kan bare krediteres én gang.

Skal fakturaen gjøres om, lager du en ny etter at kreditnotaen er på plass. Se også «Gjenta» under.

## Repeterende faktura

Fanen **Repeterende** er for abonnement, husleie og annet som faktureres likt hver gang.

- En mal har kunde, linjer, intervall (månedlig, kvartalsvis eller årlig), neste dato og eventuelt sluttdato. Malen er bare en plan. Ingenting bokføres før en faktura lages.
- I linjetekster kan du skrive `{måned}` og `{år}`. De erstattes med perioden som faktureres, så «Husleie {måned} {år}» blir «Husleie august 2026».
- Fakturaene lages hver natt av en jobb på serveren. «Lag forfalte nå» gjør det samme med en gang, og spør før noe utstedes. En mal som ligger flere perioder etter tar igjen det den har gått glipp av.
- Samme periode kan aldri faktureres to ganger, heller ikke om to kjøringer går samtidig. Feiler en kjøring, logges det, og malen forsøkes på nytt.
- En faktura som lages fra en mal er en vanlig faktura med nummer, KID, bokføring og PDF. Merk at utsendelsen fortsatt er en handling du gjør selv: kjøringen kan bare merke fakturaen for utsending.
- Maler som har kjørt kan stoppes («Stoppet») men ikke slettes.

«Gjenta» på en faktura lager en mal fra den, med samme kunde og linjer.

## Tilbud, ordre og faktura

Fanen **Tilbud og ordre** følger den kommersielle kjeden før fakturaen. Den ligger utenfor hovedboken: ingenting bokføres.

1. **Nytt tilbud.** Tilbud kan endres fritt så lenge de er utkast eller sendt. Bruk «Merk som sendt» når det er sendt, så «Akseptert» eller «Avslått». Et avslått tilbud kan ikke tas opp igjen, og portalen spør før du avslår.
2. **Lag ordre** fra et akseptert tilbud. Linjene kopieres uendret. Det kan bare finnes én ordre per tilbud, og ordren er låst.
3. **Lag faktura** fra ordren. Dette er den vanlige fakturavegen, med oppsummering først, og kjeden tilbud, ordre og faktura vises i listene.

Tilbud og ordre har hver sin nummerserie uten hull. Et avslått tilbud er historikk, ikke et hull. PDF-en for tilbud og ordre lages når du trykker «PDF», uten KID og betalingsinformasjon, fordi de ikke kan betales.

## PDF og EHF

- **Last ned PDF** gir selve fakturaen. PDF-en lagres sammen med bilaget i det øyeblikket fakturaen utstedes, og hver nedlasting kontrolleres mot kontrollsummen. Det er nøyaktig denne filen kunden får.
- **EHF-knappen** på fakturaen laster ned fakturaen som PEPPOL BIS Billing 3.0 (UBL 2.1), laget fra de låste fakturalinjene. Mva-satsen er den som gjaldt på fakturadatoen.

:::caution
EHF-filen kontrolleres mot det offisielle UBL-skjemaet, men regnmed sender den ikke selv. Transporten gjennom et PEPPOL-aksesspunkt er ikke bygget. Du kan laste ned filen og levere den gjennom en tjeneste som kan sende EHF. PEPPOLs forretningsregler (Schematron) kjøres av aksesspunktet, ikke av regnmed.
:::

## Send på e-post

«Send til kunde» sender PDF-en som vedlegg. Mottaker er kundens lagrede e-post, og du kan overstyre den per utsendelse. Svar går til selskapets egen adresse. Hver utsendelse logges med hvem som sendte hva til hvem og når, og loggen kan ikke endres. Er e-post ikke satt opp i miljøet, sier portalen fra i stedet for å late som.

## KID

KID er en betalingsreferanse som banken kan bruke til å merke innbetalingen. regnmed lager den fra fakturanummeret, med kontrollsiffer.

KID krever en OCR-/KID-avtale med banken din. Nye selskaper har sjelden en fra dag én, og en KID uten avtale er en instruks betaleren ikke kan følge. Derfor er KID **av som standard**: kryss av for avtalen i Firmaopplysninger under Administrasjon, helst før du utsteder den første fakturaen. Uten avtale står det ingen KID på fakturaen, purringen eller EHF-filen. En faktura som allerede er utstedt får aldri KID i ettertid.

## Faktura i valuta

Velg en annen **Valuta** enn NOK i fakturaskjemaet. Linjebeløpene er da i valutaens minste enhet, og hver linje regnes om til kroner med dagens kurs fra Norges Bank. Kursene hentes under Bank → Valutakurser. Fakturaen viser motverdien i NOK, og kundefordringen bokføres i kroner. En kreditnota reverserer til den opprinnelige kursen. Når innbetalingen kommer, bokføres kursforskjellen (agio) i samme operasjon som koblingen, slik at både kroner og valuta går i null. Oppgjøret gjøres fra Bank. Se [Bank](/brukerveiledning/bank/) og [Valuta](/brukerveiledning/valuta/).

## Kontantsalg

En **kontantfaktura** er for salg som er betalt ved levering. Det er et eget dokument uten KID, forfall og betalingsinformasjon, og kravet bokføres og gjøres opp i samme operasjon. Du oppgir selv hvilken konto oppgjøret går på, siden regnmed ikke gjetter hvordan noen betalte.

## Hva som ikke er bygget

- Sending av EHF gjennom et aksesspunkt (se over).
- Delfakturering: én ordre gir én faktura.
- Proratering og abonnement per bruker i repeterende maler. Malene har faste linjer.

:::note[Hva skjer bak kulissene]
Fakturanummeret hentes fra en teller som økes i samme transaksjon som bokføringen. Feiler utstedelsen, rulles begge tilbake, så verken faktura- eller bilagsnummer får hull. Fakturaen kan ikke endres i databasen heller, ikke bare i portalen.
:::
