# Utlegg og kjøregodtgjørelse

Issue #42. Ansatte og eiere legger ut privat og skal ha korrekt
refusjon — med dokumentasjonen på plass og satsene fra regelverket,
aldri fra hodet.

## Innboks-disiplinen på refusjonskrav

`expense` (migration 0026) følger bilagsinnboksens regler:

- **Innholdet er uforanderlig fra innsending.** Kvitteringen lagres
  med SHA-256 ved opplasting; trigger + kolonnegrants avviser enhver
  endring. Kjøregodtgjørelse lagrer satsene den ble beregnet med —
  en satsendring rører aldri et innsendt krav (raden er bevis).
- **Beslutninger er enveis**: innsendt → godkjent (med
  kostnadsbilaget) eller → avvist (begrunnelse påkrevd);
  godkjent → utbetalt (med utbetalingsbilaget). Ingenting
  om-besluttes, ingenting slettes — et avvist krav med begrunnelsen
  er en del av historien.
- Kvitteringen leses alltid integritetssjekket mot lagret hash.
- **Samme kvittering to ganger er ett krav, ikke to.** Innsendingen
  nekter bytes (SHA-256) som allerede står i et krav som ikke er
  avvist, og sier hvilket krav det er. Et AVVIST krav sperrer ikke:
  å sende inn på nytt med bedre forklaring er den normale veien
  tilbake. En rådgivende lås per selskap og hash
  (`pg_advisory_xact_lock`) gjør at to samtidige innsendinger av
  samme kvittering tar tur, så et dobbeltklikk ikke slipper gjennom.

## Utlegg

Innsending: rå kvittering + dato/beløp/formål. Godkjenning i ÉN
transaksjon:

1. Bilaget: debet kostnadskonto (netto), eventuell inngående mva
   skilt ut med [`split_gross`](mva.md) etter dato-riktig sats
   (debet 2710), kredit mellomregningskonto (2910 gjeld til ansatte
   som standard) for brutto.
2. **Kvitteringen kopieres inn som vedlegg på bilaget** — 
   oppbevaringsplikten dekker originaldokumentasjonen fra det
   øyeblikket kostnaden er bokført (samme mekanikk som innboksen).
3. Den enveise statusovergangen, med mellomregningskontoen lagret så
   utbetalingen debiterer samme konto.

## Kjøregodtgjørelse

km × statens sats på **kjøredatoen**, fra satsregisteret
(`km_godtgjorelse`, `km_godtgjorelse_trekkfri` — øre per km, med
kilde; docs/regelverk.md). En dato utenfor registerets dekning
avvises høyt, aldri gjettet. Ren beregning i
`regnmed-core::expenses::mileage_allowance`:

- beløp = km × sats
- trekkfri del = km × trekkfri sats (aldri over beløpet)
- **trekkpliktig del** = differansen (2026: 1,80 kr/km)

Den trekkpliktige delen skal lønnsinnberettes. A-meldingen (#46)
finnes nå, men tar ennå IKKE med kjøregodtgjørelse fra utleggene, så
delen **varsles tydelig** — i skjemaet før innsending (beløpet regnes
med satsen fra `GET …/expenses/kjoring?dato=`), på kravet og i
godkjenningssvaret — med beskjed om at den må meldes særskilt. Aldri
skjult, aldri halvhåndtert.
Godkjenning posterer hele beløpet (debet 7100 bilgodtgjørelse som
standard) mot mellomregning; bilagslinjen bærer km og sats.

## Utbetaling

`POST …/expenses/{id}/pay`: debet mellomregningskontoen fra
godkjenningen, kredit bank (1920 som standard) — én transaksjon med
statusovergangen. Når remittering (#33) kommer, mater godkjente krav
betalingslisten i stedet for et manuelt klikk; bankavstemmingen
matcher utbetalingsbilaget mot kontoutskriften som ethvert annet
bilag.

Selvgodkjenning er tillatt som standard — et enkeltpersonforetak må
kunne gjøre alt. Slår selskapet på attestering (docs/attestering.md,
#47), kan innsenderen ikke lenger godkjenne sitt eget krav; regelen
håndheves i `approve_expense`, ikke i portalen.

## Endpoints

- `GET  /companies/{id}/expenses?status=&limit=&offset=` — kravene
  nyeste først, med `own`-flagg, `total` og `counts` per status.
  `status` er en FANE: `apne` (innsendt + godkjent — pengene er ikke
  ute ennå), eller en av de lagrede statusene; tom = alle. Ukjent
  status gir 400.
- `GET  /companies/{id}/expenses/{eid}` — ett krav, med begge
  bilagene (`voucher`, `utbetalt_voucher`) og hvem som besluttet
- `POST /companies/{id}/expenses/utlegg?filename=&dato=&belop_ore=&beskrivelse=` — rå kvittering som body
- `GET  /companies/{id}/expenses/kjoring?dato=` — statens km-sats og
  den trekkfrie satsen den dagen (400 utenfor satsregisterets dekning)
- `POST /companies/{id}/expenses/kjoring`
- `GET  /companies/{id}/expenses/{eid}/receipt` (hash-sjekket)
- `POST /companies/{id}/expenses/{eid}/approve` (konto/mva_kode/motkonto valgfritt)
- `POST /companies/{id}/expenses/{eid}/reject` (note påkrevd)
- `POST /companies/{id}/expenses/{eid}/pay`

Registrering krever `UTLEGG_SKRIV_EGNE` (egne krav); godkjenning og
avvisning `UTLEGG_GODKJENN`, utbetaling `UTLEGG_UTBETAL`. Den som
bare har `UTLEGG_LES_EGNE` ser sine egne krav — et annet krav svarer
404 både i listen, på kravsiden og for kvitteringen.

Portal (**Utlegg**, `#/c/{id}/utlegg/…`): kravene først, i fanene
Åpne / Utbetalt / Avvist med tellere, sidevis fra serveren. Statusen
står i ord («Venter på godkjenning», «Godkjent, venter på
utbetaling»). «Nytt utlegg eller kjøring» (`…/utlegg/ny`) er ett
skjema med merkede felt; kvitteringen velges først (kamera eller fil
på telefon), og knappen er opptatt til svaret kommer. Hvert krav har
sin side (`…/utlegg/{id}`) med kvitteringen forhåndsvist ved siden av
beslutningen (samme bytesjekk som innboksen, `lib/preview.js`),
kontovelger, mva-valg med varsel for representasjon (mval. § 8-3),
linjene som vil bokføres, og bekreftelse med dato og bankkonto før
utbetaling. Knappene vises bare for den som har retten; etter
godkjenning eller avvisning går siden til neste krav som venter.

Bevisst utenfor v1 (issuen): diett/per-diem-satser og app-basert
kvitteringsfoto (PWA, #48).

## Where it is tested

- `regnmed-core/src/expenses.rs` — split trekkfri/trekkpliktig,
  historisk sats-invertering, null-tilfeller.
- `crates/regnmed-api/tests/grupper/expenses.rs` — end to end: kvittering
  rundtur + DB-immutabilitet, samme kvittering avvist som duplikat,
  manglende km sagt i en setning, km-satsen for skjemaet, fanene og
  tellerne, kravsiden med begge bilag, godkjenning med mva-splitt (7790/2710/
  2910-saldoer verifisert) og vedlegget på bilaget, enveis
  beslutninger (også ulovlige overganger direkte i SQL), kjøring med
  trekkpliktig-varsel og høy feil utenfor satsdekning, avvisning
  krever begrunnelse, utbetaling mot bank, kjedeverifikasjon.
- `crates/regnmed-api/tests/grupper/access.rs`
  (`an_employee_may_do_their_own`) — ansatt åpner sitt eget krav,
  får 404 på en kollegas.
- `ui/portal/src/sections/utlegg/expenseMath.test.js` — skjemaets
  kjøregodtgjørelse er kjernens (`mileage_allowance`), km-tolkning;
  `ui/portal/src/lib/preview.test.js` — typen avgjøres av bytene.
