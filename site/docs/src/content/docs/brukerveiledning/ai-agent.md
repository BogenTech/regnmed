---
title: Ta med din egen AI-agent
description: La din egen AI-modell arbeide som ansatt eller regnskapsfører i selskapet, med samme rettigheter og samme sporbarhet som et menneske.
sidebar:
  order: 44
---

Du kan la din egen AI-modell arbeide i selskapet ditt, som ansatt eller som regnskapsfører. **Regnmed hoster ingen modell og selger ingen tokens.** Du bringer agenten og nøklene, og regnmed stiller skinnen. Det holder personvern, regning og ansvar der de hører hjemme.

Teknisk oppsett står i [dokumentasjon for utviklere](/developers/). Her er det du som eier selskapet trenger å vite.

## En agent er en bruker

Agenten får ingen spesialregler. Den er en bruker med en rolle, valgt av deg blant de innebygde rollene eller dine egne. Rettigheter, sporing og tilbakekalling fungerer likt som for et menneske:

- Alt agenten gjør er knyttet til agenten i sporet, på samme måte som for en person.
- Du kan trekke tilbake tilgangen når som helst, og den virker med en gang.
- Agenten kan **aldri** få rettigheter som styrer hvem som har tilgang. Den kan gjøre jobben, men ikke utvide sin egen tilgang.

## Forslag, ikke bokføring

Standarden er forslagsrett, ikke bokføringsrett. Regnmed foreslår aldri bokføring automatisk, og en agent heller ikke.

- Agenten leser bilag og foreslår kontering. Et menneske godkjenner.
- Selskapet kan kreve at **alt en agent bokfører attesteres av et menneske, uansett beløp**, og at to agenter ikke teller som fire øyne. Du slår det på i attesteringspolicyen. Beløpsgrensen er feil målestokk for en maskin, for det som gjør et bilag verdt et blikk er at ingen har sett det.

## Oppsett i portalen

Under Administrasjon → Oppdrag og integrasjoner ligger **Agenter og integrasjoner**:

1. **Ny agent** registrerer agenten og gir den en rolle.
2. Du velger en rollemal blant fire: regnskapsfører, bokfører, ansatt og migrering. Malen styrer hvilken jobb agenten er satt opp til å gjøre.
3. Agenten logger inn med en egen klient hos innloggingstjenesten, og portalen veileder deg gjennom det. Hemmeligheten ligger i agentens egen konfigurasjon, aldri i regnmed.

**Oppgaver til en agent** lar deg gi agenten en oppgave i klartekst («Gi oppgaven»). Oppgaven kan ikke endres etter at den er gitt, så sporet viser hva agenten faktisk ble bedt om. En endret oppgave er en ny oppgave. Agenten svarer med et resultat, eller med et spørsmål når den er usikker. Du kan svare på spørsmålet («Svar til agenten»). Agenten kan ikke gi seg selv oppgaver, og det er det som gjør sporet verdt å lese.

Under **Siste endringer fra agenter og integrasjoner** ser du hva agentene har gjort.

## Hva agenten ikke gjør

- Agenten kan ikke få tilgang til å styre medlemmer, oppdrag, integrasjoner eller selskapsinnstillinger.
- En agent som er usikker, har en konkret handling: den stiller et spørsmål i stedet for å gjette.
- Ferdige programfiler for MCP-serveren i utgivelsene er ikke på plass ennå. Serveren bygges fra kildekoden og kjører på din egen maskin.

Se også [Teknisk dokumentasjon: Agent](/teknisk/agent/) og [Integrasjoner](/teknisk/integrations/).

<img src="/screens/integrasjoner-light.png" alt="Agenter og integrasjoner" width="1440" height="900" loading="lazy" />
<img src="/screens/integrasjoner-dark.png" alt="Agenter og integrasjoner" width="1440" height="900" loading="lazy" />
