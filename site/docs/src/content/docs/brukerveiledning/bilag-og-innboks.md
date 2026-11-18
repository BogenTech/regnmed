---
title: Bilag og innboks
description: Fra kvittering og leverandørfaktura til bokført bilag, med bilagsforslag, e-post-inn og attestering.
sidebar:
  order: 5
---

Bilag-seksjonen er den daglige veien mellom virksomheten og den som fører regnskapet. Dokumentasjon lastes opp med en gang den finnes, og et menneske gjør hvert dokument til et bokført bilag eller avviser det med en begrunnelse.

Seksjonen har fanene **Innboks**, **Behandlede**, **Bokførte bilag** og **Innstillinger**.

## Last opp

Det er tre veier inn i innboksen:

- **Dra filen** inn i feltet «Dra fakturaer og kvitteringer hit», eller bruk knappen for å velge den.
- **Ta bilde av kvittering.** På telefon åpner knappen kameraet. På en datamaskin åpner den filvelgeren. Er du uten nett, legges bildet i en kø på telefonen og sendes når nettet er tilbake. Hver fil får en kontrollsum som serveren sjekker, så et nytt forsøk aldri blir to bilag.
- **E-post.** Se under.

Innholdet kan ikke endres etter opplasting. Samme fil nektes så lenge en kopi ligger i innboksen som ubehandlet eller bokført. En kopi som ble avvist blokkerer ikke: avvisningen svarte på den opplastingen, og samme kvittering sendt på nytt er et nytt dokument.

![Innboksen med dokumenter som venter](/screens/bilag-light.png)

## E-post-inn

Leverandører og ansatte kan sende kvitteringer på e-post uten å logge inn. Under **Innstillinger** finner du kortet «E-post-inn» med selskapets adresse, som ser ut som `bilag-<navn>-<tilfeldig>@…`.

- Adressen gir bare rett til å **levere** noe i innboksen, aldri til å lese. Navnedelen er lesbar, den tilfeldige halen kan ikke gjettes.
- Lekker adressen, trykker du «Ny adresse». Den nye finnes med en gang, og den gamle slutter å ta imot i samme øyeblikk. Den gamle kan ikke komme tilbake.
- Under godkjente avsendere legger du inn en full adresse (`post@leverandor.no`) eller et helt domene (`@leverandor.no`) med «+ avsender». Jokertegn finnes ikke.
- En e-post fra en godkjent avsender med vedlegg blir innboksdokumenter med en gang. Avsenderadressen står som den som leverte dokumentet.
- En e-post fra en **ukjent avsender** havner i **karantene**, under «Venter på avgjørelse». Den lagres helt, men gir ingen dokumenter før en administrator velger «Slipp inn» (valgfritt med avsenderen lagt til på listen) eller «Avvis» med en begrunnelse. Hverken stille import eller stille sletting: begge ville vært verre.
- En e-post uten vedlegg avvises, men loggføres med begrunnelse.

E-post bokfører aldri noe. Er e-post-inn ikke satt opp i miljøet ditt, sier portalen det i stedet for å vise en adresse som ikke tar imot noe.

## Bilagsforslag

Åpner du et dokument og trykker **Forslag**, fyller regnmed ut bokføringsskjemaet fra det dokumentet selv sier. Forslaget er alltid et forslag: det finnes ingen automatisk bokføring, heller ikke over eller under et beløp. Du ser tallene, retter det som er galt og bokfører selv.

Hvor tallene kommer fra, står over skjemaet, og hvert felt har en begrunnelse («Hvorfor»):

| Kilde | Presisjon |
| --- | --- |
| EHF-faktura | Eksakt: tallene står i filen |
| PDF med tekstlag | Heuristikk, bygget på kontrollsifrene (organisasjonsnummer, KID, kontonummer) |
| Ren tekstfil | Samme heuristikk |
| Foto | Samme heuristikk over lest tekst, hvis tekstleseren er satt opp |
| Annet, eller skannet dokument uten tekst | Ingenting foreslås, og det står |

Kontoforslaget er kontoen samme leverandør sist ble bokført på. Er leverandøren funnet, følger forfallsdato og KID fra fakturaen med inn i skjemaet.

## Bokfør

Velg dokumentet, se på det til venstre og fyll ut **Bokfør**:

1. **Dato på bilaget** og **Tekst**.
2. **Hvordan ble det betalt?** Faktura som skal betales, betalt fra bankkonto, eller betalt kontant.
3. **Leverandør** hvis det er en faktura som skal betales. Her kan du også «Legg til leverandør».
4. **Beløp inkl. mva (kr)** og **Mva**.
5. **Hva var det? (konto).** Skriv for eksempel «kontor», «reise» eller et kontonummer.
6. Sjekk linjene under «Dette bokføres:». Trenger du mer, åpner «Avansert: rediger linjene» debet og kredit per linje, også med avdeling og prosjekt.
7. Trykk «Bokfør».

Bokføringen skjer i én operasjon: bilaget posteres, dokumentet kopieres inn som vedlegg til bilaget (oppbevaringsplikten begynner her), og innboksposten merkes bokført. Feiler noe, for eksempel at perioden er låst, forblir dokumentet ubehandlet og ingenting er bokført.

## Avvis

«Avvis dokumentet» krever en begrunnelse, og begrunnelsen går tilbake til den som leverte det. Dokumentet og begrunnelsen ligger igjen som en del av historien. Verken bokføring eller avvisning kan angres: et dokument som er avgjort, forblir avgjort. Skal noe rettes, gjør du det med et nytt bilag, se [Hovedbok og kontoplan](/brukerveiledning/hovedbok/).

## Attestering

Større virksomheter skiller mellom den som mottar en kostnad, den som godkjenner den og den som bokfører eller betaler. Attestering er valgfri. Uten den fungerer alt som beskrevet over.

Administratoren slår den på under Bilag → Innstillinger → Attestering:

- **Krev attestering.**
- **Beløpsgrense (kr).** Tom betyr at alle bilag må attesteres.
- **Hvem attesterer.** Alle med bokføringstilgang, eller en utpekt person.
- **Alt en agent bokfører må attesteres av et menneske.** Gjelder uansett beløp, for agenter, som er automatiske brukere.

Slik virker det: dokumentet får en attestering med «Godkjenn» eller «Avvis» (avvisning krever begrunnelse). Før bokføring kreves godkjenning når beløpet er på eller over grensen, og **den som attesterte kan ikke selv bokføre**. Kolonnen Attestering i innboksen viser status. Alle beslutninger, også en avvisning som senere ble en godkjenning, ligger igjen med hvem og når.

Med aktiv attestering kan bilag over grensen heller ikke bokføres for hånd i Hovedbok, fordi det ville vært en bakdør forbi kontrollen. Betalingslister har samme regel: den som godkjenner må være en annen enn den som opprettet listen.

## Ansattes utlegg og kvitteringer

En ansatt som har lagt ut for noe, sender inn kvitteringen med kortet «Send inn kvittering» øverst i Bilag (på telefon åpner det kameraet). Det er et krav om refusjon, ikke et bilag å bokføre, og det følger sin egen flyt med godkjenning og utbetaling.

:::note[Hva skjer bak kulissene]
Dokumentets innhold og kontrollsum kan ikke endres etter ankomst, og kontrollsummen sjekkes igjen hver gang filen lastes ned. Beslutningene i innboksen går bare én vei, og kan ikke endres selv ved en feil i programmet.
:::
