---
title: Hovedbok og kontoplan
description: Kontoplanen, drill-down per konto, manuell bilagsføring og hvorfor bokførte bilag aldri endres.
sidebar:
  order: 6
---

Hovedbok-seksjonen er der regnskapet kan sees og føres direkte. Det meste i regnmed bokføres som en følge av et dokument: faktura, innboks, lønn, utlegg. Hovedbok er den frie veien, med de samme vaktene.

Seksjonen har fanene **Bilag**, **Nytt bilag**, **Kontoplan**, **Periodisering** og **Kassaoppgjør**.

<img src="/screens/hovedbok-light.png" alt="Hovedboken med kontoer og posteringer" width="1440" height="900" loading="lazy" />
<img src="/screens/hovedbok-dark.png" alt="Hovedboken med kontoer og posteringer" width="1440" height="900" loading="lazy" />

## Bilag: selve boken

Fanen Bilag viser alle bilag med debet- og kreditlinjene, nyeste øverst. Du kan velge år, søke i fritekst og filtrere på bilag uten vedlegg. Søk og år står i adressen, så du kan bokmerke eller dele et utsnitt.

Et bilag har en egen side med linjene, vedleggene (last ned, eller «Legg ved dokument» hvis noe mangler) og tilbakeføring.

## Kontoplan

Fanen Kontoplan viser selskapets kontoer med **Saldo** og antall posteringer. Saldoen beregnes alltid fra posteringene.

- **Standardkontoplanen** (Skatteetatens liste med 254 kontoer) er søkbar, men ligger ikke inne i selskapet. En konto blir selskapets første gang du legger den til eller bokfører på den. Det holder saldobalansen ren.
- **«Legg til»** på en standardkonto henter inn nummer og navn. For en **«Ny konto»** utenfor standarden skriver du nummer og eget navn.
- **«Endre»** gir deg navn og aktiv/deaktivert. Kontonummeret er permanent, fordi posteringene og kontrollsummene refererer til det.
- En **deaktivert** konto tar ikke imot nye posteringer, men historikken består. Ingenting slettes. «Vis deaktiverte» tar dem med i listen.

Velg en konto for å se posteringene for året, fra inngående saldo og med løpende saldo. Hver postering lenker til bilaget.

## Manuelt bilag

1. Åpne fanen **Nytt bilag**.
2. Sett **Dato** og **Tekst**.
3. Legg inn **Linjer** med Konto, Debet og Kredit. «Legg til linje» gir flere. Du kan sette Avdeling og Prosjekt per linje.
4. Velger du en **Mva**-kode på en linje, regnes beløpet som brutto. Mvaen skilles ut på 2710 (inngående) eller 2700 (utgående), og koden settes på nettolinjen.
5. Sjekk «Dette bokføres (mva skilt ut)». Debet og kredit må gå i null.
6. Bokfør.

Knappen er opptatt til svaret kommer, siden et bokført bilag aldri kan slettes. Beløp kan skrives med mellomrom og komma, for eksempel «12 500,00».

Reglene som gjelder alle veier inn gjelder også her: perioden må være åpen, kontoene må være aktive, dimensjoner må finnes, og reskontrokontoer krever en part. Har selskapet **aktiv attestering** og bilaget når grensen, nektes manuell føring, og du henvises til innboksen. Et bilag med dokument hører uansett hjemme i innboksen.

## Bokførte bilag kan aldri endres

Bokføringsloven §9 forbyr å endre eller slette bokførte opplysninger. I regnmed er det ikke bare en regel i portalen:

- Programmet har ingen funksjon som endrer et bilag.
- Databasen avviser endring og sletting av bilag og posteringer, også fra programmet selv.
- Hvert bilag bærer en kontrollsum som bygger på det forrige. Endres noe i ettertid, brytes kjeden, og kontrollen «Kontroller at ingenting er endret» viser det. Se [teknisk dokumentasjon om hovedboken](/teknisk/ledger/).

### Retting: tilbakeføring

Skal et bilag rettes, åpner du bilagets side og trykker **«Tilbakefør bilaget»**. Det bokfører et nytt bilag der hver linje er speilvendt, koblet til originalen. Deretter fører du det riktige bilaget.

- Reskontroposter lukkes automatisk mot speilbildet i samme operasjon, så en tilbakeført faktura ikke blir liggende som åpen post.
- Datoen må ligge i en åpen periode.
- Et bilag som allerede er tilbakeført, og en tilbakeføring, kan ikke tilbakeføres igjen.
- Bilag som en annen flyt eier tilbakeføres ikke her, og du får beskjed om hvor det rettes: faktura rettes med kreditnota, og det samme gjelder purring, utlegg, betalingsliste, anlegg, lønn, aksjeeierbok, periodisering, årsavslutning og importert historikk.

Banklinjer som var koblet til det tilbakeførte bilaget frigjøres og kan avstemmes på nytt mot det riktige bilaget.

## Periodisering og kassaoppgjør

**Periodisering** fordeler kostnad og inntekt over månedene de hører hjemme i. **Kassaoppgjør** bokfører dagsoppgjør fra et kassasystem, der kassadifferansen alltid blir et eget bilag. Begge ender som ordinære bilag og følger de samme reglene som alt annet. Mer om periodisering i [teknisk dokumentasjon](/teknisk/periodisering/) og kassaoppgjør i [teknisk dokumentasjon](/teknisk/kontantsalg/).
