---
title: Rapporter og årsavslutning
description: De lovpålagte spesifikasjonene, nøkkeltall, balansedokumentasjon, periodelåsing og årsavslutning.
sidebar:
  order: 37
---

## Rapportene

Under **Rapporter** er hver rapport en egen fane: Saldobalanse, Resultat, Balanse, Budsjett, Prosjekt, Kontospesifikasjon, Reskontro, Bokføringsspesifikasjon, Balansedokumentasjon, Revisjon og SAF-T. Du velger år, og år, fane og filtre står i adressen, så tilbake-knappen og delte lenker havner der du var.

Alle rapportene er spørringer over hovedboken, aldri lagrede rapporter. En rapport for 2026 tatt ut i dag og en tatt ut om fem år viser de samme tallene, fordi bilagene ikke kan endres.

| Fane | Hva den viser |
| --- | --- |
| Saldobalanse | per konto: inngående, debet, kredit og utgående |
| Resultat | resultatregnskap, gruppert etter NS 4102 (klasse 3–8), med filter på avdeling eller prosjekt |
| Balanse | eiendeler og gjeld/egenkapital (klasse 1–2) pluss udisponert resultat, så balansen går i null |
| Kontospesifikasjon | alle posteringer per konto med løpende saldo og henvisning til bilaget |
| Reskontro | kunde- og leverandørspesifikasjon: inngående saldo, posteringer og utgående saldo per part |
| Bokføringsspesifikasjon | alle bilag i rekkefølgen de ble bokført, med alle linjer |
| SAF-T | eksport av regnskapet i Skatteetatens standardformat |

Henvisningen til bilaget er en lenke, så sporbarheten fra spesifikasjon til bilag er ett klikk. De lange spesifikasjonene er delt i sider, og «Last ned CSV» gir **hele perioden** uansett hvilken side du står på. Filen er semikolonseparert med desimalkomma og åpnes rett i et norsk regneark.

## Nøkkeltall

På **Oversikt** finner du nøkkeltallene: resultat hittil i år mot samme dato i fjor, månedskolonner, et likviditetsbilde og de to neste mva-fristene. Likviditeten er alltid for nå, uavhengig av valgt år.

## Balansedokumentasjon

Bokføringsloven krever at hver balansepost kan dokumenteres ved periodeslutt: kontoutskrift, varetellingsliste, avstemming mot oppgaver. På fanen **Balansedokumentasjon** registrerer du avstemmingen per konto og periode («Registrer avstemmingen»), med en forklaring og eventuelt et vedlegg.

- Saldoen leses av regnmed, ikke oppgitt av deg.
- En ny avstemming er en ny rad, og den nyeste gjelder. En avstemming som kunne skrives om i ettertid er en påstand, ikke dokumentasjon.
- Vedlegget lagres med et fingeravtrykk som kontrolleres hver gang det lastes ned.
- Bokføres kontoen videre etter avstemmingen, viser rapporten forskjellen («bokført X etter avstemming»). Det er ikke det samme som udokumentert.

Revisjonsrapporten tar balansekontoer med saldo som mangler dokumentasjon ved siste låste periode som **avvik**.

## Periodelåsing

Under Administrasjon → **Periode** låser du perioder du er ferdig med (ajourhold). Bilag datert i en låst periode avvises, både av regnmed og av databasen selv. Rettelser føres i en åpen periode som reverserende bilag.

- Det vanlige valget er en knapp: lås til og med slutten av forrige måned. Du kan også velge en annen dato («Lås til og med»), men aldri en dato frem i tid. Begge bekreftes først.
- Låsen har full historikk. Å flytte den bakover er en **gjenåpning**, som bare administrator kan gjøre, og den står i historikken.
- Å legge ved dokumentasjon på et bilag i en låst periode er lov. Å endre bilaget er det ikke.

## Årsavslutning

Under Administrasjon → Periode ligger kortet **Årsavslutning**. Det bokfører resultatdisponering og skattekostnad som ett ordinært bilag:

| Linje | Konto |
| --- | --- |
| Skattekostnad (utelates når den er 0) | 8300 debet |
| Betalbar skatt | 2500 kredit |
| Disponering av årets resultat | 8800 debet (overskudd) |
| Opptjent egenkapital | 2050 kredit |

Slik bruker du det:

1. Velg år med pilene. Fjoråret er valgt som standard, og resultatet hentes straks.
2. Oppgi skattekostnaden. Regnmed regner den ikke ut, fordi skattemessig resultat er noe annet enn regnskapsmessig resultat. **0 er et gyldig svar, men må oppgis**: et tomt felt regnes ikke som 0.
3. «Se over avslutningen» viser bilaget uten å bokføre det.
4. «Avslutt {år} og lås året» bokfører det. Bekreftelsen nektes hvis hovedboken har endret seg siden du så oversikten.

Avslutningen låser året i samme transaksjon som bilaget. Et år kan bare avsluttes fra 1. januar året etter, og bare én gang. Har mva-låsen allerede gått forbi 31.12, kreves administrator, fordi bilaget da føres i en låst periode. Utsatt skatt er ikke bygget.

Se også [Teknisk dokumentasjon: Rapporter](/teknisk/rapporter/), [Årsavslutning](/teknisk/arsavslutning/) og [Perioder](/teknisk/perioder/).
