---
title: Periodisering og budsjett
description: Fordele kostnader og inntekter over månedene de hører til, og følge opp mot budsjett.
sidebar:
  order: 36
---

## Periodisering

Husleie betalt for et helt år, forsikringspremier og årsabonnementer skal ikke ligge som en pukkel i betalingsmåneden. Da viser månedsrapportene noe som ikke er drift. Under Hovedbok → **Periodisering** lager du en plan som fordeler beløpet over månedene det hører til.

En plan består av kildebilaget, beskrivelse, resultatkonto, balansekonto, **nettobeløp**, fra- og til-måned, eventuelle dimensjoner og et notat.

:::caution
Periodisering flytter kostnad og inntekt, **aldri merverdiavgift**. Avgiften følger salgsdokumentet og trekkes fra i terminen fakturaen hører til, uansett hvordan kostnaden fordeles i resultatet. Beløpet i planen er derfor netto, og linjene som bokføres har ingen mva-kode. Avgiften står på kildebilaget, som er ført på vanlig måte.
:::

- Retningen ligger i fortegnet: en forskuddsbetalt kostnad har positivt beløp, en uopptjent inntekt negativt.
- Regnmed bokfører én måned om gangen, med bilag datert månedens siste dag. En måned kan ikke føres to ganger.
- Kjøringen skjer automatisk den 1. for måneden som nettopp endte. Du kan også kjøre den selv.
- En feil på én plan stopper ikke de andre. Feilen logges med årsak.

## Budsjett

Budsjettet er det eneste tallet i regnmed som er en mening, og det behandles deretter. Du finner det under Rapporter → **Budsjett**.

**Utkast og fastsatt.** Et utkast kan redigeres fritt: linjer legges til og tall skrives om. Når du **fastsetter** budsjettet fryses versjonen, og hvem som fastsatte og når lagres. En revisjon er en **ny versjon** for samme år. Derfor kan en avviksrapport alltid navngi hva den måler mot, for eksempel «Budsjett 2026 v1 (fastsatt)». Utkast kan forkastes, fastsatte budsjetter kan ikke.

**Slik skriver du det.** Linjene er konto per måned, i samme fortegn som resultatrapporten: inntekt positiv og kostnad positiv. Du skriver «100 000» for planlagt salg, ikke «−100 000». Bare resultatkontoer (klasse 3–8) kan budsjetteres. Likviditetsbudsjett finnes ikke.

Du kan fylle ut fra fjorårets tall med et påslag eller fradrag i prosent.

### Avviksrapporten

Avviket er faktisk minus budsjett, per konto, gruppert som i resultatregnskapet. Rapporten viser hittil i år til og med valgt måned, hele året, og de tolv månedene per konto. Regnmed tolker ikke fortegnet for deg: et positivt avvik på en inntektskonto er bedre enn planlagt, mens det på en kostnadskonto er dyrere. Kontoer som bare finnes på én side tas med.

Rapporten bruker fastsatt versjon som standard, ellers nyeste utkast, og sier alltid hvilken versjon og status den bruker.

Se også [Teknisk dokumentasjon: Periodisering](/teknisk/periodisering/) og [Budsjett](/teknisk/budsjett/).
