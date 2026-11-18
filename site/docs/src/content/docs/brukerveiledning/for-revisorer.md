---
title: For revisorer
description: Skrivebeskyttet tilgang, verifikasjonsrapporten og hvordan du etterprøver regnskapet uten å stole på regnmed.
sidebar:
  order: 41
---

Løftet til revisorer er produktets kjerne: **ikke stol på oss, verifiser.** Denne siden beskriver hva revisorrollen kan, og hvordan du kontrollerer regnskapet selv.

## Tilgangen

Et revisjonsfirma registreres som byrå av typen revisor, med autorisasjon verifisert mot Finanstilsynets register (se [For regnskapsførere og byråer](/brukerveiledning/for-regnskapsforere/)). Klienten sender et oppdrag, og når det er godtatt får du rollen `revisor`:

- Du kan **lese alt**: rapporter, reskontro, bilag, vedlegg, forankringer, og lønnsopplysninger som revisjonen krever.
- Du kan **ikke endre noe**. Alle endrende handlinger krever bokføring eller admin.
- Når klienten avslutter oppdraget, forsvinner tilgangen umiddelbart.

## Verifikasjonsrapporten

Under Rapporter → **Revisjon** kjører regnmed alle kontrollene systemet kan gjøre på sitt eget regnskap. En feilet kontroll blir en **AVVIK-linje** i rapporten. Den skjuler aldri dokumentet.

| Kontroll | Hva den beviser |
| --- | --- |
| Hash-kjede fra genesis | hvert bilag er regnet ut på nytt fra lagret innhold, og lenkene mellom bilagene er hele |
| Rapportgrunnlag | månedssummene rapportene leser er lik summen av postene, konto for konto og måned for måned |
| Bilagsvedlegg | vedleggenes innhold stemmer med lagret fingeravtrykk |
| Ekstern forankring | forankrede kjedetopper finnes fortsatt i kjeden, og lagrede røtter lar seg regne ut igjen |
| Reskontro mot hovedbok | summen av partenes poster er lik kontoens saldo, konto for konto |
| Balansekontroll | alle posteringer summerer til null øre |
| Periodelåsing | gjeldende lås og omfanget av låsehistorikken (informasjon) |
| Regelverkssatser | ingen overvåket sats er eldre enn den burde være |
| Balansedokumentasjon | balansekontoer med saldo som mangler dokumentasjon ved siste låste periode. **Dette er et avvik.** |
| Dokumentasjon | hvor mange bilag som mangler vedlegg i regnmed (informasjon, aldri avvik: dokumentet kan ligge i et annet medium) |
| Importert historikk | hvilke eksterne SAF-T-filer regnskapet er bygget fra, med full innholdshash |

Rapporten viser også hver ekstern forankring som dekker selskapet (tidspunkt, rot, vitner) og kjedetoppen da rapporten ble laget.

### Last ned rapporten

Rapporten kan lastes ned som tekst. Den er deterministisk, så du kan arkivere den. Den ender med en **prosedyre for uavhengig etterprøving**: gå gjennom kjeden fra start med den dokumenterte formelen, sammenlign med de offentlige forankringene, og hash kildefilene fra importen og sammenlign.

## Etterprøv selv

Du trenger ikke stole på at regnmed sier «OK».

- Kjeden: hvert bilag lagrer `SHA-256(forrige hash || kanonisk innhold)`. Formelen og serialiseringen er dokumentert og testet mot en fastlåst sjekksum (se [Teknisk dokumentasjon: Hovedboken](/teknisk/ledger/)).
- Forankringen: hver natt fryses alle selskapenes kjedetopper under én Merkle-rot som publiseres åpent. Du kan kontrollere at en forankret kjedetopp er med i roten uten å stole på API-et (se [Teknisk dokumentasjon: Forankring](/teknisk/anchoring/) og [Verifiser regnskapet ditt](/brukerveiledning/verifiser-regnskapet/)).
- Kilden er åpen (AGPL), så du kan lese hvordan kontrollene er laget.

## Begrensninger regnmed sier fra om

- Settet av vedlegg på et bilag er ikke med i hash-kjeden, fordi dokumentasjon legitimt kommer etter bokføringen. Hvert vedlegg har likevel sitt eget fingeravtrykk som kontrolleres.
- En ny eksport av samme periode fra et gammelt system (andre bytes, samme innhold) fanges ikke av importloggen.

Se også [Teknisk dokumentasjon: Revisjon](/teknisk/revisjon/).

<img src="/screens/revisjon-light.png" alt="Verifikasjonsrapporten" width="1440" height="900" loading="lazy" />
<img src="/screens/revisjon-dark.png" alt="Verifikasjonsrapporten" width="1440" height="900" loading="lazy" />
