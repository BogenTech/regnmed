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
`regnmed-core::utlegg::kjoregodtgjorelse`:

- beløp = km × sats
- trekkfri del = km × trekkfri sats (aldri over beløpet)
- **trekkpliktig del** = differansen (2026: 1,80 kr/km)

Den trekkpliktige delen skal lønnsinnberettes — a-melding er bevisst
utsatt (#46), så delen **varsles tydelig** ved registrering, i
listen og i godkjenningssvaret. Aldri skjult, aldri halvhåndtert.
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

- `GET  /companies/{id}/expenses` — alle krav, med `own`-flagg
- `POST /companies/{id}/expenses/utlegg?filename=&dato=&belop_ore=&beskrivelse=` — rå kvittering som body
- `POST /companies/{id}/expenses/kjoring`
- `GET  /companies/{id}/expenses/{eid}/receipt` (hash-sjekket)
- `POST /companies/{id}/expenses/{eid}/approve` (konto/mva_kode/motkonto valgfritt)
- `POST /companies/{id}/expenses/{eid}/reject` (note påkrevd)
- `POST /companies/{id}/expenses/{eid}/pay`

Registrering krever skrivetilgang (egne krav); godkjenning,
avvisning og utbetaling krever bokforing eller admin. Portal:
**Utlegg**-seksjon med begge skjemaer, kravlisten med statusløp og
trekkpliktig-badge, kvitteringsnedlasting og beslutningsknappene.

Bevisst utenfor v1 (issuen): diett/per-diem-satser og app-basert
kvitteringsfoto (PWA, #48).

## Where it is tested

- `regnmed-core/src/utlegg.rs` — split trekkfri/trekkpliktig,
  historisk sats-invertering, null-tilfeller.
- `crates/regnmed-api/tests/grupper/expenses.rs` — end to end: kvittering
  rundtur + DB-immutabilitet, godkjenning med mva-splitt (7790/2710/
  2910-saldoer verifisert) og vedlegget på bilaget, enveis
  beslutninger (også ulovlige overganger direkte i SQL), kjøring med
  trekkpliktig-varsel og høy feil utenfor satsdekning, avvisning
  krever begrunnelse, utbetaling mot bank, kjedeverifikasjon.
