---
title: Migrering fra et annet system
description: Flytt regnskapet fra Tripletex, Fiken, Visma, Conta og andre med SAF-T, kontakter og åpne poster.
sidebar:
  order: 42
---

Det norske regelverket krever at alle regnskapssystemer kan eksportere **SAF-T Financial**. Derfor kan regnmed flytte regnskapet fra de fleste systemer, blant annet Visma, Tripletex, Fiken, Conta, PowerOffice og Unimicro. På en tom selskapsoversikt finner du kortet «Kom fra et annet system?».

Importen krever administrator.

## Før du begynner

- Eksporter SAF-T Financial fra det gamle systemet. Noen systemer lager én fil per regnskapsår.
- Selskapet må være tomt i regnmed, eller bare inneholde importert historikk.
- Behold det gamle systemet til du har kontrollert saldobalansen.

## Slik fungerer importen

Hver fil importeres i **én transaksjon**: feiler noe, rulles hele filen tilbake.

- Historikken blir ekte bilag, bokført på vanlig måte i en egen import-journal, med det gamle systemets transaksjons-id bevart i teksten. De er med i hash-kjeden fra start og kontrolleres som alt annet.
- **Åpningsbalansen** fra filen blir ett bilag datert dagen før historikken starter. Den må summere til null. En delvis eksport avvises med differansen oppgitt.
- Reskontro importeres forsiktig: en konto flagges som kunde eller leverandør bare hvis hver linje har riktig part. Blandede kontoer importeres uten kobling, og hvert tilfelle står som en advarsel.
- Ukjente mva-koder utelates med en advarsel.

## Flere år: én fil om gangen

Har det gamle systemet én fil per år, importerer du dem **eldste først**. Importen er åpen så lenge hovedboken bare inneholder importerte bilag. Det første vanlige bilaget stenger den for godt.

En oppfølgingsfil må fortsette nøyaktig der historikken stoppet. Åpningssaldoene avstemmes konto for konto før noe skrives. Avviker de, avvises importen med kontoen og differansen i øre. Kortet heter «Importer neste årsfil» så lenge importen er åpen.

Samme fil kan aldri importeres to ganger. Regnmed lagrer et fingeravtrykk av hver importert fil, og kan vise listen over filer hovedboken er bygget fra. Én begrensning: en ny eksport av samme periode (andre bytes, men samme innhold) fanges ikke.

## Kontoplanveiviseren

Bruker det gamle systemet en annen kontoplan enn standard (NS 4102), for eksempel femsifrede kontoer, går importen i to steg:

1. Regnmed leser filen uten å skrive noe og foreslår en kobling til standardkontoer: firesifrede kontoer kobles til seg selv, lengre tall kuttes, kortere fylles ut, ellers brukes kontonavnet.
2. Du går gjennom forslagene, retter dem og fullfører koblingen. Det er ditt valg som importeres. Kontoer du kobler til samme standardkonto slås sammen. En halvferdig kobling feiler høylytt i stedet for å importere feil.

## Uten SAF-T

Kan ikke det gamle systemet eksportere SAF-T, kan en administrator bokføre en manuell **åpningsbalanse** på et tomt regnskap. Linjene må summere til null.

## Kontakter og åpne poster fra CSV

SAF-T flytter hovedboken, men ikke alt. Kontaktinformasjon (adresse, e-post, kontonummer) og åpne poster flyttes med CSV. Du finner dette i Reskontro: «Importer fra et annet system».

- Regnmed leser kolonnene fra **overskriftene**, ikke fra en profil per leverandør. En fil den ikke forstår feiler med kolonnene den faktisk så.
- **Kontakter** importeres idempotent: samme kunde blir ikke to. Et numerisk kundenummer blir partsnummeret.
- **Åpne poster** har en forhåndsvisning som sjekker alt uten å bokføre. Der **vinner restbeløp over fakturabeløp**, og retningen bestemmes av om parten er kunde eller leverandør, ikke av fortegnet i filen.
- Postene bokføres som ett bilag mot en motkonto. Reskontrokontoen må stå på null først, og regnmed sier fra med den faktiske saldoen hvis den ikke gjør det.

Anbefalt rekkefølge: kontakter først, så åpningsbalanse uten kundefordringer og leverandørgjeld, så åpne poster. Etterpå er reskontrosaldoen lik summen av postene fordi det er de samme radene.

## Ikke bygget

Direkte kobling til de andre systemenes API-er er ikke bygget. Den krever avtale og nøkler per leverandør, og er en mulig videreutvikling.

Se også [Teknisk dokumentasjon: Migrering](/teknisk/migration/).
