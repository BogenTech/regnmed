---
title: Lønn og a-melding
description: Ansatte, skattekort, lønnskjøring, lønnsslipp og a-melding. Hva som er bygget, hva som bare er prøvd i test, og hva regnmed nekter å gjøre.
sidebar:
  order: 30
---

Lønn finner du i menyen under **Lønn**. Les denne siden først: lønn i regnmed er ikke en komplett lønnsmodul, og dokumentasjonen sier det rett ut i stedet for å love mer.

## Hva som virker

- fastlønn og timelønn (timelønn fra låste timer)
- trekk fra skattekortet: prosent, trekktabell og frikort
- arbeidsgiveravgift per sone
- feriepenger, med avsetning og avgift på avsetningen
- lønnsslipp som PDF
- a-melding som lages og valideres mot Skatteetatens skjema, og som kan sendes

## Hva som ikke virker ennå

- **A-melding i produksjon.** Innsendingen er bygget og prøvd mot Skatteetatens testmiljø. I produksjon venter den på Altinn-systembruker. Selskapet må inntil videre levere a-meldingen på annen måte.
- **Erstatningsmelding ved korrigering.** En korrigert lønnskjøring er et reverserende bilag og en ny kjøring i hovedboken. Hvordan det skal se ut i a-meldingen er ikke bygget.
- **Sone Ia.** Se under «Arbeidsgiveravgift».
- **Endringsbestilling av skattekort.** I dag henter du kortet på nytt med en knapp.

## Ansatte

Under **Ansatte** registrerer du hver ansatt med «Registrer ansatt». Listen viser fødselsdato, aldri fødselsnummer. Nummeret lagres fordi a-meldingen krever det, men leses bare der det trengs.

For at a-meldingen skal kunne lages må arbeidsforholdet være komplett: virksomhet (underenhet), yrkeskode, stillingsprosent, timer per uke for en full stilling, arbeidstidsordning, ansettelsesform og siste lønnsendring. Listen viser «mangler N» per ansatt, og et skjema lar deg fylle ut feltene. Regnmed fyller aldri inn noe for deg. Det som mangler, mangler synlig.

Når en ansatt slutter, setter du sluttdato i ansattlisten. Uten den rapporteres vedkommende som ansatt hver måned.

**Koble til portalbruker.** En ansatt leser egen lønnsslipp når personen er koblet til en bruker. Det skjer ved at du inviterer den ansatte fra ansattlisten med e-postadressen vedkommende logger inn med. Koblingen settes når invitasjonen løses inn. Du velger aldri fra en liste, og regnmed gjetter aldri på navn.

## Skattekort

Under **Skattekort** henter regnmed kortene fra Skatteetaten, så du slipper å taste prosenten. Det krever at selskapet har godtatt kundevilkårene (Administrasjon → Firmaopplysninger). Svaret er ofte klart etter noen sekunder. Er det ikke det, bruker du «Hent svaret».

Bare kort for **inneværende år** hentes. Kort for neste år hentes i januar, før årets første kjøring.

Regnmed setter bare det lønnskjøringen kan regne riktig:

| Kortet sier | Regnmed |
| --- | --- |
| Prosent over 10,5 måneder | settes |
| Prosent over 12 måneder (blant annet kildeskatt) | settes, merket 12 måneder |
| Trekktabell | settes |
| Frikort uten beløpsgrense | settes til ingen trekk |
| Frikort med beløpsgrense | settes med grensen |
| Annet antall måneder | beholdes, med en merknad |
| Ikke skattekort | beholdes, med en merknad |

Det som beholdes får en merknad med grunnen, og lønnskjøringen sier fra om hver ansatt der kortet ikke ble satt. Ingen kjøring går stille forbi et kort som ikke kom med.

:::note
En frikortgrense på 0 kr settes ikke. Kortet står med begrunnelsen, og trekket må føres for hånd. Skatteetatens eget testsvar gjør at 0 ikke kan tolkes sikkert.
:::

## Kjøre lønn

Under **Kjør lønn** velger du måned og ser en beregning før noe bokføres («Dette bokføres for …»). Når du bekrefter, blir lønnskjøringen **ett ordinært bilag** i hovedboken, i én transaksjon:

- lønn, forskuddstrekk og netto til utbetaling
- feriepenger: utbetalt denne måneden, og månedens avsetning
- arbeidsgiveravgift, og avgift på avsatte feriepenger

Netto bokføres som skyldig lønn, ikke som bank. Selve utbetalingen gjør du som en betalingsliste (se Bank → Betalinger). Lønnskjøring og utbetaling er to atskilte, sporbare handlinger.

Samme måned kan ikke kjøres to ganger. En feil rettes med et reverserende bilag og en ny kjøring, som ellers i hovedboken.

Hvis selskapet ble opprettet før 11. november 2026 og mangler noen av lønnskontoene, nekter regnmed kjøringen før noe bokføres og sier hvilke kontoer som mangler. Legg dem til under Hovedbok → Kontoplan.

## Trekk

- **Prosenttrekk** fra skattekortet.
- **Tabelltrekk** regnes med Skatteetatens egen trekkrutine, sjekket mot hele den publiserte tabellfilen for 2026. Et år uten innlagte konstanter stopper kjøringen i stedet for å bruke fjorårets.
- **Feriepenger er trekkfrie** i utbetalingsåret, og det trekkes halv skatt i desember. Begge deler gjelder bare kort over 10,5 måneder. Kort over 12 måneder trekker fullt hver måned.
- Trekket **rundes ned til hele kroner**, siden a-meldingen tar hele kroner.

Lønnsslippen sier hvordan trekket ble regnet, for eksempel «35,00 %» eller «tabell 8000».

## Arbeidsgiveravgift

Satsen hentes fra satsregisteret per sone, med kilde på hver rad. Avgiften regnes én gang per kjøring på samlet grunnlag og rundes ned til hele kroner, slik Skatteetaten gjør.

**Sone Ia avvises.** Den reduserte satsen gjelder bare til et fribeløp er brukt opp, og fribeløpet forbrukes også av støtte regnmed ikke ser. Å regne flat sats ville gitt feil avgift til Skatteetaten, så regnmed sier fra i stedet for å tilnærme.

## Feriepenger

Feriepengesatsen settes per ansatt (lovens minimum 10,2 %, eller 12,5 % over 60 år). Hver kjøring avsetter feriepenger og avgift på dem. Utbetalte feriepenger trekker ned gjelden. De er ingen ny kostnad.

Hvis feriepengegjeld stammer fra noe annet enn lønnskjøringene (åpningsbalanse, import, manuelt bilag), kan regnmed ikke knytte den til en ansatt. Kjøringen sier fra med beløpet. Den delen må avsettes manuelt.

## Lønnsslipp

Hver ansatt har en lønnsslipp som PDF per kjøring. En ansatt med portalbruker ser bare sin egen. Andre krever lønnsrettigheter (se [Tilgang og roller](/brukerveiledning/tilgang-og-roller/)).

## A-melding

Fra en lønnskjøring lager regnmed en a-melding som er validert mot Skatteetatens offisielle skjema. Meldingen følger **utbetalingsmåneden**, ikke lønnsmåneden. Alle arbeidsforhold rapporteres hver måned til og med sluttmåneden, også en måned uten lønn. En måned uten kjøring får en egen melding med bare arbeidsforholdene.

Regnmed bruker bare koder fra Skatteetatens publiserte lister, og **nekter** når et felt som reglene krever mangler. Et oppdiktet felt blir godtatt av Skatteetaten og er feil hos NAV, og det er verre enn en melding som ikke sendes. Feilmeldingen sier hva som mangler.

Se også [Teknisk dokumentasjon: Lønn](/teknisk/lonn/).

<img src="/screens/lonn-light.png" alt="Lønn i regnmed" width="1440" height="900" loading="lazy" />
<img src="/screens/lonn-dark.png" alt="Lønn i regnmed" width="1440" height="900" loading="lazy" />
