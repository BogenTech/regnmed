---
title: Tilgang og roller
description: Hvem som kan gjøre hva i selskapet ditt, hvordan du inviterer folk, lager egne roller, og hva «ingen tilgang» betyr.
sidebar:
  order: 39
---

Regnmed skiller mellom hvem du er og hva du får gjøre. Innloggingen beviser hvem du er. **Hva du får gjøre bestemmes av regnmed, per selskap.** Den som sitter med 60 klienter kan ha en annen rolle i hvert enkelt selskap.

## De innebygde rollene

| Rolle | Hva den gir |
| --- | --- |
| `ansatt` | Selvbetjening: egne timer, eget utlegg, egen lønnsslipp og kvitteringsfoto. Ikke tilgang til hovedboken. |
| `les` | Lese regnskapet. Ikke lønn. |
| `revisor` | Som `les`, pluss lønnsopplysningene revisjonen krever. Følger av et revisjonsoppdrag og kan ikke gis direkte. |
| `bokforing` | `les` og alt som endrer hovedboken, pluss lønn. |
| `admin` | `bokforing`, pluss å styre selskapet og hvem som slipper inn. |

Rollene finnes med klartekstforklaring under Administrasjon → **Brukere og roller**.

## Invitere folk

Under Administrasjon → Brukere og roller inviterer du en person til en **e-postadresse** med en rolle. Adressen må være den personen logger inn med.

- Personen får en e-post. Den inneholder ingen hemmelig lenke, bare en vei til portalen. Invitasjonen løses inn når adressen logger inn.
- Svaret avslører aldri om adressen allerede har en bruker hos regnmed.
- Er e-posten ikke sendt (for eksempel fordi utsendelsen er nede), blir invitasjonen likevel opprettet, og du får beskjed. Du kan sende på nytt.
- Så lenge selskapet bare har ett medlem, foreslår Oversikt noen vanlige profiler: lønnsmottaker (`ansatt`), økonomiansvarlig (`bokforing`) og medeier (`admin`).
- Den **eksterne regnskapsføreren** inviteres ikke. Hen kommer inn gjennom et oppdrag (se [For regnskapsførere og byråer](/brukerveiledning/for-regnskapsforere/)), som følger avtalen og kan avsluttes samme dag.

Selskapet kan aldri bli stående uten administrator: den siste administratoren kan verken fjerne seg selv eller bytte rolle. Medlemmer deaktiveres, de slettes aldri, og endringene står i en logg.

## Egne roller

Trenger du noe annet, for eksempel en controller som ikke ser lønn, lager du en egen rolle. Du setter den sammen av enkeltrettigheter. Rutenettet viser hva hver rettighet lar deg gjøre, for eksempel «Se alles timer».

- Navn på innebygde roller er reservert.
- Rettigheter som styrer **hvem som har tilgang** kan bare gis av admin, og kan ikke delegeres til en egen rolle. Det står ved siden av avkryssingsboksen.
- Roller deaktiveres, de slettes aldri.
- Ukjente rollenavn gir ingenting. Regnmed velger alltid den strengeste tolkningen.
- Har en person flere veier inn (direkte medlemskap og oppdrag), får personen alle rettighetene samlet.

## Hva «404» betyr for deg

Når du ikke har tilgang til et selskap, svarer regnmed som om selskapet ikke finnes. Du får «ikke funnet», aldri «forbudt». Det er bevisst: en som ikke har tilgang skal ikke lære at selskapet finnes.

Får du derimot «forbudt», har du tilgang, men ikke nok. Typisk eksempel er en revisor som prøver å endre noe.

## Abonnement og tilgang

Et sperret abonnement stopper endringer, aldri lesing, eksport eller styring av tilgang. Se [Abonnement og priser](/brukerveiledning/abonnement-og-priser/).

## Plattformrolle

Regnmed selv har ingen vei inn i noen hovedbok. Plattformrollene `support` og `systemadmin` kan bare se administrative stamdata (brukere, medlemskap, kunderegistre), er tidsbegrenset og logges, og loggen er synlig for selskapet det gjaldt. Du finner den under Administrasjon → Brukere og roller, i kortet «Leverandørtilgang».

Se også [Teknisk dokumentasjon: Tilgang](/teknisk/auth/).
