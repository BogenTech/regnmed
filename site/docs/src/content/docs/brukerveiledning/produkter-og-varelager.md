---
title: Produkter og varelager
description: Produktregisteret som gjør fakturering raskere, og det enkle varelageret for deg som selger varer.
sidebar:
  order: 35
---

## Produktregisteret

Under **Produkter** ligger **Produktregisteret**. Et produkt har nummer, navn, salgspris, mva-kode og inntektskonto. Bruk «Nytt produkt» for å legge inn et.

- **Nummeret kan ikke endres senere.** Det står på dokumenter og i lagerhistorikken. Navn, pris og kontoer kan endres.
- Produkter slettes aldri, bare deaktiveres. Et deaktivert produkt nektes på nye dokumenter, men beholder historikken.
- Inntektskontoen må være en konto i klasse 3 (3000–3999). Regnmed sier fra ved registreringen og ikke først på fakturaen.

### Kopieres ved utstedelse

Når du bruker et produkt på en faktura, et tilbud eller en mal, kopieres beskrivelse, pris, konto og mva-kode inn på linjen. Å endre produktet etterpå rører aldri et dokument som allerede er utstedt. Du kan overstyre hver verdi på linjen, for eksempel en forhandlet pris. Det du lar stå tomt kommer fra registeret.

## Varelager

Varelageret er valgfritt, og gjelder produkter som du merker som lagerført. **Beholdning lagres aldri.** Den er summen av en logg som bare kan utvides, på samme måte som en kontosaldo er summen av bilag.

Det finnes tre typer bevegelser:

- **Varekjøp** legger du inn selv med anskaffelseskost per enhet («Registrer varekjøp eller justering»).
- **Salg** føres automatisk av fakturaen for hver linje med lagerført produkt. En kreditnota legger varen tilbake på lager.
- **Justering** krever en forklaring.

Verdien beregnes etter gjennomsnittsmetoden. Negativ beholdning vises, ikke skjules.

### Varetelling

«Se over tellingen» viser justeringene og verdibilaget uten å bokføre noe. Når du bekrefter, bokføres justeringene og et bilag mot bokført lagersaldo i én transaksjon. Standardkontoene er 1460 for lager og 4390 for lagerendring, og du kan velge andre.
