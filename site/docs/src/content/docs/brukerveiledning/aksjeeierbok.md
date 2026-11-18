---
title: Aksjeeierbok og aksjonærregisteroppgaven
description: Føre aksjeeierboken som loven krever, registrere utbytte og forberede RF-1086. Hva som kan leveres, og hva regnmed nekter.
sidebar:
  order: 38
---

To ting blandes ofte sammen, og regnmed holder dem fra hverandre:

1. **Aksjeeierboken** er et register selskapet fører selv (aksjeloven § 4-5). Den har verdi hver dag.
2. **Aksjonærregisteroppgaven (RF-1086)** er den årlige innrapporteringen til Skatteetaten, med frist 31. januar.

Boken er kilden til oppgaven. Ingenting tastes inn to ganger. Fra juni 2026 kan oppgaven bare leveres gjennom et sluttbrukersystem. Altinn.no og papir er avviklet. Selskaper registrert i Euronext VPS rapporterer via VPS og er unntatt.

## Aksjeeierboken

Under **Aksjonærer** finner du **Aksjeeierbok**, **Hendelser** og **Utbytte**.

- **Ny aksjonær** registrerer en person eller et foretak. Identiteten kan ikke endres etterpå. Et bytte er en ny aksjonær, ikke en redigering. Kontaktinformasjon kan endres, og aksjonærer slettes aldri.
- **Ny hendelse** registrerer aksjer inn eller ut (stiftelse, kjøp, salg, overføring og så videre). En overdragelse blir to hendelser i én transaksjon, med motpart begge veier.
- **Eierandelen lagres aldri.** Den er summen av hendelsene frem til en dato, på samme måte som en kontosaldo er summen av bilag. En rettelse er en motsatt hendelse.

**Personvern.** Boken viser fødselsdato, ikke fødselsnummer. Fødselsnummeret leses bare ett sted, når oppgaven bygges.

## Utbytte

«Nytt utbyttevedtak» registrerer **ett vedtak**, ikke ett beløp per eier. Hver aksjonærs utbytte er beholdningen på beslutningsdatoen ganger beløpet per aksje, så delene kan ikke avvike fra helheten. «Oversikt over utbyttet» viser fordelingen før du bokfører. Utbytte bokføres mot utbyttegjeld i samme transaksjon.

## Aksjonærregisteroppgaven

Fanen **Aksjonærregisteroppgaven** lager hovedskjema (RF-1086) og ett underskjema per aksjonær, validert mot Skatteetatens offisielle skjema. Forhåndsvisningen viser tallene og hindringene.

:::caution
**Regnmed leverer bare transaksjonstyper det er sikkert på.** Skatteetaten har ikke publisert hele kodelisten for transaksjonstyper. En feil type flyter videre inn i aksjonærens egen skatteoppgave og endrer inngangsverdi og skjermingsgrunnlag. Regnmed leverer derfor bare de verifiserte kodene, og **nekter høylytt** for resten i stedet for å gjette. Forhåndsvisningen viser fortsatt tallene og forteller hva som hindrer innsending.
:::

Innsendingen er bygget og prøvd mot Skatteetatens testmiljø, der en oppgave er blitt godkjent. Regnmed lagrer det som sendes før det sendes, og kvitteringen med forsendelses-id står på innsendingen for alltid. Svaret fra Skatteetaten hentes med en knapp. For at innsending skal virke må selskapet ha godtatt kundevilkårene.

Ikke bygget ennå: korrigering av en levert oppgave, og automatisk henting av svaret. Regnmed nekter en ny innsending for et år som allerede er levert.

Se også [Teknisk dokumentasjon: Aksjeeierbok](/teknisk/aksjonaer/).
