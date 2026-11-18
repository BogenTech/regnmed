---
title: Verifiser regnskapet ditt
description: Hva «manipulasjonssikker» betyr i praksis, hvordan du kontrollerer kjeden, og hva forankring beviser.
sidebar:
  order: 45
---

De fleste regnskapssystemer ber deg stole på dem. Regnmed lar deg kontrollere.

## Tre lag mot endring

Hovedboken kan bare utvides. Det er sikret på tre nivåer:

1. **Regnskapsreglene.** En feil rettes med et motsatt bilag (reversering), aldri ved å redigere. Både rettelsen og feilen blir stående.
2. **Databasen.** Databasen avviser endring og sletting av bilag, og applikasjonen har bare lov til å legge til og lese.
3. **Kryptografi.** Hvert bilag lagrer et fingeravtrykk (SHA-256) av forrige bilag pluss sitt eget innhold. Endrer noen ett bilag, stemmer ikke lenger fingeravtrykkene etter det.

Det betyr at manipulering kan **oppdages**, også av noen med full tilgang til databasen.

## Kjeden mot forankringen

En som kontrollerer databasen kunne i teorien skrive om hele kjeden og regne ut alle fingeravtrykk på nytt. Derfor finnes **ekstern forankring**:

- Hver natt fryser regnmed alle selskapenes kjedetopper under én felles Merkle-rot. Roten røper ingenting om det enkelte selskapet.
- Roten publiseres på en åpen adresse (`/anchors` på API-et), uten innlogging. Hver uavhengig kopi, en revisor, en overvåkingsjobb, din egen cron, er et vitne regnmed ikke kan nå.
- Når en tidsstempeltjeneste er satt opp, sendes roten også dit, og du får et signert bevis for at roten fantes senest på et gitt tidspunkt (RFC 3161).
- Forankringene lagres i tabeller som bare kan utvides, på samme måte som hovedboken.

Skulle noen skrive om forankret historikk, motsier det bevis de ikke kan trekke tilbake.

## Slik kontrollerer du

**I portalen.** På Oversikt finner du kortet **Forankring** og knappen «Verifiser kjeden mot forankringen». Den kjører hele kontrollen: kjeden fra start, fingeravtrykk på vedlegg og hver forankret kjedetopp mot den levende kjeden.

**Verifikasjonsrapporten.** Under Rapporter → Revisjon får du en rapport med elleve kontroller og en prosedyre for uavhengig etterprøving. Se [For revisorer](/brukerveiledning/for-revisorer/).

**Selv.** Formelen for fingeravtrykkene er dokumentert og testet, og kildekoden er åpen. En forankring kan kontrolleres frakoblet mot den offentlige roten uten å stole på API-et.

## Hva dette ikke beviser

- Det beviser ikke at bilagene stemmer med virkeligheten. Det beviser at de ikke er endret etter bokføring.
- Settet av vedlegg på et bilag er ikke med i kjeden, fordi dokumentasjon legitimt kommer etter bokføringen. Hvert vedlegg har sitt eget fingeravtrykk, kontrollert ved hver nedlasting.
- Forankringen dekker kjedetoppene på forankringstidspunktet. Det som er bokført etter siste nattlige forankring er beskyttet av kjeden, men ikke ennå av en ekstern rot.

Se også [Teknisk dokumentasjon: Hovedboken](/teknisk/ledger/) og [Forankring](/teknisk/anchoring/).
