---
title: Abonnement og priser
description: Hva regnmed koster, hvordan prøvetid og frist virker, og hvorfor et utløpt abonnement aldri tar regnskapet ditt som gissel.
sidebar:
  order: 43
---

## Prisene

Pakkene og prisene redigeres i plattformkonsollen, og den gjeldende prislisten står alltid på [regnmed.no/priser](https://www.regnmed.no/priser/). Den viser hver pakke med pris, grenser og det som følger med, og en pris som er vedtatt fram i tid vises med datoen den gjelder fra. Prisene er uten mva, og brukere koster aldri ekstra.

Pakkene skiller på størrelse:

- **Bilag i året.** En pakkegrense stopper aldri bokføringen. Går du over grensen, får du beskjed i portalen og forslag om en pakke som passer. Bilag regnmed fører selv (avskrivninger, periodiseringer, lønnskjøringer, purregebyr og årsavslutning) telles ikke.
- **Ansatte i lønn** og **integrasjoner og agenter.** Disse har en fast grense. Når pakken er full, avvises en ny ansatt eller en ny agent med en melding som sier hvilken pakke du er på.

Under **Administrasjon → Abonnement** ser du pakken du er på og hvor mye du bruker av grensene. Betaler du med faktura, bytter du pakke selv der: en større pakke gjelder med en gang, og prisforskjellen for inneværende måned faktureres; en mindre pakke gjelder fra den 1. i neste måned. Du kan ikke bytte til en pakke med færre plasser for ansatte eller integrasjoner enn du bruker. Betaler du med kort, tar du kontakt med oss for å bytte. Prøveperioden har ingen grenser.

Prisene er data med dato og kilde. En ny pris gjelder fra en dato som er satt fram i tid, og et selskap som står på en eldre pakke beholder den og prisen sin, også om abonnementet har vært ute en stund og startes igjen.

**Byråer betaler ikke.** All fakturering er per selskap, og hver klient har sitt eget abonnement. Byråavtaler med rabatt er ikke bygget.

## Prøvetid, frist og sperre

Statusen beregnes alltid ut fra datoene og lagres aldri:

| Status | Når | Hva skjer |
| --- | --- | --- |
| Prøvetid | nytt selskap uten abonnement, under 30 dager gammelt | alt virker, og et banner sier når prøvetiden løper ut |
| Aktiv | et abonnement dekker dagens dato | alt virker |
| Frist | dekningen eller prøvetiden er ute for mindre enn 14 dager siden | alt virker, og et banner varsler sperredatoen |
| Sperret | fristen er ute | **endringer** avvises med en forklaring |

## Sperren tar aldri regnskapet som gissel

Bokføringsloven pålegger deg å oppbevare regnskapet, og hele poenget med regnmed er at du kan verifisere det og ta det med deg. Derfor:

- **Lesing virker alltid.** Rapporter, spesifikasjoner og nedlastinger.
- **Eksport virker alltid.** SAF-T-eksporten er veien ut, og den er åpen nettopp når du er misfornøyd nok til å dra.
- **Styring av selskapet virker alltid:** tilgang, oppdrag, integrasjoner og firmaopplysninger, så du kan slippe inn den som skal rydde opp.
- **Å dokumentere det som allerede er bokført virker alltid.** Du kan legge ved dokumentasjon på bokførte bilag.

Det sperren stopper er det som endrer regnskapet, for eksempel å bokføre, fakturere eller laste opp nye dokumenter i innboksen.

## Betaling

Faktura med KID er standard. Regnmed utsteder fakturaen fra sin egen fakturamotor, og innbetalingen kommer inn via bank som alle andre betalinger. Betaling med kort er bygget (Stripe, med abonnement som fornyes til du sier opp), men slås på per miljø. Er det ikke slått på i ditt miljø, sier portalen det. Kortdata lagres aldri hos regnmed, bare en referanse og de siste fire sifrene.

Under Administrasjon → **Abonnement** ser du status, plan og betalinger. Har du kort tilgjengelig, kan du legge til kort og starte abonnementet selv.

## Oppfølging av ubetalt faktura

Livssyklusen rundt en ubetalt abonnementsfaktura er automatisk:

1. Fakturaen sendes på e-post med PDF.
2. Forfaller den uten betaling, følger en påminnelse og så en purring, etter den samme trappen regnmed bruker for sine kunder. Inkasso overlates til bevillingshavere.
3. Dekningen avsluttes 30 dager etter forfall når en purring er sendt. Den vanlige fristen på 14 dager kommer oppå, så reell sperre er rundt dag 44.
4. Betaler du alle utestående fakturaer, tegnes abonnementet på nytt.

En **oppsigelse** gjenopprettes aldri automatisk. Bare en avslutning som maskinen selv gjorde, kan gjenopprettes.

Se også [Teknisk dokumentasjon: Abonnement](/teknisk/abonnement/).
