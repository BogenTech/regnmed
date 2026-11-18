---
title: Abonnement og priser
description: Hva regnmed koster, hvordan prøvetid og frist virker, og hvorfor et utløpt abonnement aldri tar regnskapet ditt som gissel.
sidebar:
  order: 43
---

## Prisene

Alt er inkludert i begge planene. Brukere koster aldri, og det er ingen bilagsgrenser eller moduler som må kjøpes til.

:::note
Nye planer (Liten, Standard og Stor etter antall bilag) er foreslått, men ikke innført. Tabellen under er det som gjelder i dag. Se [priser](https://www.regnmed.no/priser/).
:::


| Plan | Pris per selskap | Forskjellen |
| --- | --- | --- |
| Basis | 49 kr per måned | Selvbetjent. Dokumentasjonen er supporten. |
| Standard | 99 kr per måned | Samme funksjoner, pluss e-postsupport. |

Prisene er oppgitt uten mva. Mva legges på etter registreringen til selskapet som fakturerer, og beregnes ikke som en fast sats. Skillet mellom planene er støttekanalen og ingenting annet. Ingen funksjoner er låst bak Standard.

Prisene er data med dato og kilde. En prisendring gjelder nye avtaler, og eksisterende kunder beholder sin plan.

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
