# Attestering: godkjenningsflyt før bokføring og betaling

Issue #47. Større SMB-er skiller hvem som MOTTAR en kostnad, hvem som
GODKJENNER den, og hvem som BOKFØRER eller BETALER den. Intern kontroll
som en førsteklasses flyt — ikke en UI-konvensjon, men en regel
transaksjonene selv håndhever.

Flyten er **valgfri**. Uten registrert policy oppfører systemet seg
nøyaktig som før (#21 bilagsinnboks, #33 betalingsliste, #42 utlegg):
selvgodkjenning er tillatt, ingenting krever et ekstra skritt. Det er
selskapet som slår kontrollen på.

## Policyen — datert, append-only, aldri gjettet

`attestation_policy` (migration 0030) er append-only historikk der
nyeste rad gjelder — samme mønster som mva-terminordningen (#51). En
endring er en ny rad, så historikken alltid viser hvilken kontroll som
gjaldt da et bilag ble bokført.

| Felt | Betydning |
| --- | --- |
| `aktiv` | Slår kontrollen på. Av = v1-oppførselen, uendret. |
| `belopsgrense_ore` | Innboksbilag med **debetsum ≥ grensen** krever attestering. `NULL` = alle bilag. |
| `attestant_person_id` | Utpekt attestant. `NULL` = alle med bokføringstilgang kan attestere. |

Bare admin kan sette policyen (`POST …/attestering/policy`); alle med
tilgang kan lese den, inkludert revisor.

## Beslutningssporet

`attestation` er insert-only, med samme disiplin som innboksens egne
beslutninger: **nyeste beslutning gjelder, ingenting slettes**. Et
bilag som først avvises og siden godkjennes bærer begge radene, med
hvem og når — nøyaktig det et ettersyn spør etter. En avvisning uten
begrunnelse er ikke en beslutning (databasesjekk, ikke bare kode).

Attestering avgjør bare **ubesluttede** bilag: når dokumentet er
bokført eller avvist i innboksen, er attesteringsvinduet lukket.

## Hva som håndheves, og hvor

Alle tre reglene ligger i `regnmed-db`, inne i de samme transaksjonene
som gjør arbeidet — aldri bare i portalen:

1. **Bokføring fra innboksen** (`bokfor_inbox_document`): med aktiv
   policy krever et bilag på eller over beløpsgrensen en gjeldende
   `godkjent`-attestering. Mangler den, avvises bokføringen; er
   nyeste beslutning `avvist`, avvises den med begrunnelsen. Og den
   som attesterte kan **ikke selv bokføre** — fire øyne.
2. **Betalingsliste** (`approve_run`): godkjenneren må være en annen
   person enn oppretteren. Kjøringen bærer nå `created_by_person`
   (identitet, ikke bare visningsnavn); kjøringer opprettet før
   migrasjonen kan ikke godkjennes under aktiv policy — listen lages
   på nytt. Fire øyne på penger ut var alt strukturelt forberedt i
   #33 (opprettelse og godkjenning har alltid vært separate,
   auditerte handlinger); dette gjør det til en regel.
3. **Utleggskrav** (`approve_expense`): innsenderen kan ikke godkjenne
   sitt eget krav. Selvgodkjenning var uttrykkelig tillatt i #42 v1 —
   attestering legger seg oppå, som planlagt.

Grensen måles på **debetsummen i bilagsutkastet**, ikke på dokumentets
størrelse eller noe lagret felt: kontrollen gjelder beløpet som faktisk
bokføres.

## Endpoints

- `GET  /companies/{id}/attestering/policy` — gjeldende policy + full historikk
- `POST /companies/{id}/attestering/policy` — ny policyrad (admin)
- `GET  /companies/{id}/members` — attestant-kandidater (admin)
- `POST /companies/{id}/inbox/{doc}/attester` — `{godkjent, note?}`
  (bokføringstilgang; avvisning krever notat)
- `GET  /companies/{id}/inbox/{doc}/attestering` — beslutningssporet
  (alle tilgangsnivåer, også revisor)

Innboks-listingen bærer `attestering` og `attestert_av` per dokument,
så køen kan vises uten et kall per bilag.

## Portal

Bilag-seksjonen åpner med kortet **Til attestering**: bilagene som
venter, med Godkjenn/Avvis, og policyskjemaet (av/på, beløpsgrense,
utpekt attestant) under. Innboksen har fått en attesteringskolonne
(attestert / avvist / venter). Betalingslistens rader har alltid vist
`oppretter / godkjenner` — teksten sier nå at de må være forskjellige
når kontrollen er på.

## Tester

`crates/regnmed-api/tests/grupper/attestering.rs` kjører hele historien mot en
ekte database: fritt bilag uten policy, policy satt (og nektet for
ikke-admin), bilag under grensen rett gjennom, bilag over grensen
stoppet, feil attestant nektet, avvisning som stopper bokføring,
avvisning uten notat nektet, godkjenning som legger seg over avvisningen
med begge radene i sporet, attestant som ikke får bokføre selv,
regnskapsfører som får det, betalingsliste nektet for oppretteren og
godkjent av den andre, og utleggskrav nektet for innsenderen.

## Bevisst utenfor v1

Flertrinns godkjenningskjeder og beløpsmatriser (ett steg, én grense
holder), delegeringsregler, og attestering av andre måltyper enn
innboksbilag — betalingslister og utlegg håndheves på sine egne
handlinger, uten egne attesteringsrader. `target_kind` i sporet er
allerede en kolonne, så utvidelsen er en migrasjon, ikke en omskriving.
