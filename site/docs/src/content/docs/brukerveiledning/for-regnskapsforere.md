---
title: For regnskapsførere og byråer
description: Registrere byrået, få autorisasjonen verifisert, ta imot oppdrag og slippe inn medarbeidere.
sidebar:
  order: 40
---

Regnmed er bygget slik at byrået og klienten arbeider i **samme levende regnskap**. Du trenger ingen eksport og ingen kopi. Byrået registreres én gang, og klientene gir byrået tilgang gjennom et oppdrag.

## Registrere byrået

Ved første innlogging velger du «Regnskapsfører eller revisor». Du oppgir organisasjonsnummeret, og regnmed slår det opp i Enhetsregisteret og sjekker **autorisasjonen i Finanstilsynets register**.

- Byrået opprettes bare hvis organisasjonsnummeret har aktiv autorisasjon av riktig type (regnskapsfører eller revisor).
- Kontrollen feiler på den trygge siden: er registeret utilgjengelig, regnes du som ikke verifisert. Ingen blir byrå fordi et oppslag tilfeldigvis feilet.
- Slettede enheter og enheter under konkurs eller avvikling avvises.
- Tidspunktet og kilden til verifiseringen lagres på byrået.
- Er organisasjonsnummeret allerede registrert som byrå, nektes registreringen. Den første registrerte blir admin, og alle andre kommer inn ved invitasjon.

Byrået er **gratis**. All fakturering er per selskap, og byrået har ingen abonnement. Hver klient har sin egen prøvetid og sitt eget abonnement, se [Abonnement og priser](/brukerveiledning/abonnement-og-priser/).

Et verifisert byrå kan også føre sitt eget regnskap som selskap i regnmed. Det er to ulike registreringer.

## Oppdrag

Et oppdrag er avtalen som gir byrået tilgang til en klient.

1. **Klienten finner byrået.** Under Administrasjon → Oppdrag og integrasjoner søker klientens admin i katalogen med «Finn et byrå»: på navn eller organisasjonsnummer og type. Bare byråer med verifisert autorisasjon vises, med antall aktive klienter som størrelsessignal.
2. **Klienten sender en forespørsel.** Det kan bare én ventende forespørsel være per byrå og selskap. Klienten kan trekke den tilbake så lenge byrået ikke har svart.
3. **Byrået svarer.** Under **Forespørsler** i byråvisningen godtar eller avslår en admin i byrået. Å godta åpner oppdraget i samme steg, og tilgangen virker med en gang uten ny innlogging.
4. **Klienten kan avslutte oppdraget når som helst.** Tilgangen opphører umiddelbart. Historikken står igjen.

Tilgangen følger oppdragets type. Et regnskapsføreroppdrag gir byrået rollen med rett til å føre regnskapet, et revisoroppdrag gir skrivebeskyttet tilgang.

## Klienter

Byråvisningen har **Klienter**: aktive klienter, og avsluttede oppdrag. Det er porteføljen din. Du bytter mellom klienter uten å logge inn på nytt.

## Medarbeidere

Under **Medlemmer** i byråvisningen inviterer en admin medarbeidere på e-postadresse.

- `admin` styrer byrået: medlemmer, invitasjoner og avgjørelser om oppdrag.
- `ansatt` arbeider i byrået: ser forespørsler og klienter, men avgjør ingenting.
- Hver aktive medarbeider kommer til **alle** byråets klienter. Å slippe noen inn i byrået er å slippe dem inn i porteføljen. Tilgang per klient finnes ikke ennå.
- Byrået kan aldri miste sin siste aktive admin.
- Medlemmer deaktiveres, aldri slettes, og det står i en logg hvem som slapp hvem inn.

## Meldinger

Under **Meldinger** snakker du med klienten, kolleger og eventuelle agenter i selskapet i samme portal.

Se også [Teknisk dokumentasjon: Marketplace](/teknisk/marketplace/).

<img src="/screens/byra-light.png" alt="Byråvisningen" width="1440" height="900" loading="lazy" />
<img src="/screens/byra-dark.png" alt="Byråvisningen" width="1440" height="900" loading="lazy" />
