---
title: Vanlige spørsmål
description: "Korte svar på det folk lurer mest på: retting av bilag, eksport, mva-melding, a-melding, tilgang og pris."
sidebar:
  order: 46
---

## Regnskapet

**Kan jeg slette eller endre et bilag?**
Nei. Hovedboken kan bare utvides, og det er sikret i regelverket, i databasen og med kryptografi. Du retter en feil med et motsatt bilag, og begge blir stående. Det er også slik god bokføringsskikk fungerer. Se [Verifiser regnskapet ditt](/brukerveiledning/verifiser-regnskapet/).

**Hvorfor har noen bilagsnumre ingen hull?**
Bilagsnummer er sammenhengende per journal og regnskapsår. En ikke-bokført forhåndsvisning bruker aldri opp et nummer.

**Kan jeg ta regnskapet med meg ut?**
Ja, når som helst. SAF-T-eksport finnes under Rapporter → SAF-T og virker også om abonnementet er sperret. Se [Abonnement og priser](/brukerveiledning/abonnement-og-priser/).

**Hvordan kommer jeg fra et annet system?**
Med SAF-T-import, pluss CSV for kontakter og åpne poster. Se [Migrering](/brukerveiledning/migrering/).

**Hvorfor nekter regnmed å bokføre i en periode?**
Perioden er låst. Rettelsen føres i en åpen periode. Se [Rapporter og årsavslutning](/brukerveiledning/rapporter-og-arsavslutning/).

## Det offentlige

**Kan regnmed sende mva-meldingen for meg?**
Regnmed lager mva-meldingen og lar deg laste den ned som fil. Direkte innsending til Skatteetaten er ikke på plass ennå.

**Kan regnmed sende a-meldingen?**
Meldingen lages og valideres, og innsendingen er prøvd mot Skatteetatens testmiljø. I produksjon venter den på Altinn-systembruker, så foreløpig leverer du a-meldingen på annen måte. Se [Lønn og a-melding](/brukerveiledning/lonn/).

**Kan regnmed levere aksjonærregisteroppgaven?**
Delvis. Regnmed leverer bare transaksjonstypene det er sikre koder for, og nekter resten i stedet for å gjette. Se [Aksjeeierbok](/brukerveiledning/aksjeeierbok/).

**Hva med skattemelding og næringsspesifikasjon?**
Det er kartlagt, men ikke bygget. Innsendingen går via ID-porten, ikke via maskin-til-maskin-tilgang. Regnskapet kan eksporteres som SAF-T, og resultat og balanse kan tas ut under Rapporter.

**Hva med e-faktura?**
Regnmed lager EHF-faktura (PEPPOL BIS Billing 3.0) av hver faktura og leser mottatt EHF inn i innboksen som et bokføringsforslag. Selve transporten via et aksesspunkt er ikke på plass ennå.

## Tilgang

**Hvorfor får jeg «ikke funnet» på et selskap jeg vet finnes?**
Du har ikke tilgang. Regnmed svarer som om selskapet ikke fantes, så ingen skal kunne lære hvilke selskaper som finnes. Be en administrator om en invitasjon. Se [Tilgang og roller](/brukerveiledning/tilgang-og-roller/).

**Kan regnmed selv se regnskapet mitt?**
Nei. Det finnes ingen vei for leverandøren inn i noen hovedbok. Plattformrollene ser bare administrative stamdata, er tidsbegrenset og logges, og loggen er synlig for deg.

**Hva er forskjellen på en invitasjon og et oppdrag?**
En invitasjon gir en person tilgang til selskapet ditt. Et oppdrag gir et byrå tilgang, følger avtalen og kan avsluttes samme dag.

## Pris og drift

**Hva koster det?**
Det kommer an på pakken, som følger størrelsen på regnskapet. Den gjeldende prislisten står på [regnmed.no/priser](https://www.regnmed.no/priser/). Prisen er per selskap og uten mva, og brukere koster aldri ekstra.

**Hva skjer hvis jeg slutter å betale?**
Etter prøvetid, frist og sperre stopper det som endrer regnskapet. Lesing, eksport og styring virker alltid.

**Kan jeg kjøre regnmed selv?**
Ja. Kildekoden er åpen under AGPL-3.0. Kjører du den selv, gjelder lisensen. Kunder av den hostede tjenesten trenger ingen lisens.

**Kan jeg bruke min egen AI-agent?**
Ja, med rollen du velger og et menneske som godkjenner bokføring hvis du ber om det. Se [Ta med din egen AI-agent](/brukerveiledning/ai-agent/).
