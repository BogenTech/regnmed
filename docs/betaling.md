# Betalingsliste og remittering (pain.001)

Issue #33. Vi leser bankens side (camt.053/CSV, docs/bank.md); dette
lukker sløyfen ut: åpne leverandørposter blir en betalingsliste, listen
blir en ISO 20022 pain.001-fil som lastes opp i enhver nettbank, og
utbetalingen bokføres med reskontro-lukking i én transaksjon.

## Kjøringens livsløp — bevis med enveis status

`payment_run` (migration 0029), håndhevet av trigger:

- **utkast → godkjent**: å lage listen og å godkjenne den for eksport
  er SEPARATE handlinger med hver sin audit-linje (created_by /
  approved_by). Med attestering aktiv (docs/attestering.md, #47) MÅ
  godkjenneren være en annen person enn oppretteren — kjøringen bærer
  `created_by_person` for nettopp det.
  Godkjenningen renderer pain.001-filen, lagrer den med SHA-256
  og fryser den — nedlastingen er alltid integritetssjekket.
- **godkjent → utbetalt**: «Registrer utbetalt» posterer ETT
  utbetalingsbilag (debet hver leverandørpost's konto med parten,
  kredit bank) og lukker hver posts reskontro-rest med en match-rad —
  alt i én transaksjon. Bankimportens debet kobler seg deretter mot
  det bilaget gjennom den ordinære motoren — sirkelen lukkes med
  maskineriet som allerede finnes.
- **utkast → annullert**: synlig historikk, aldri sletting. Godkjente
  kjøringer annulleres ikke — filen finnes; hva som skjedde i banken
  er et oppgjørsspørsmål.

Radene på kjøringen (`payment_run_item`) er en KOPI av kreditordata
ved opprettelsen (navn, kontonummer, KID/melding) — filen kan
reproduseres fra radene for alltid, uansett hva partsregisteret sier
senere. Beløp valideres mot postens ÅPNE rest (beregnet, aldri
lagret); KID valideres MOD10/MOD11.

## Kontonummer

`party.bank_account` (og selskapets eget, fra Firmaopplysninger) er
redigerbar kontaktinfo, MOD11-validert før lagring
(`regnmed-core::pain001::gyldig_kontonummer` — samme sykliske vekter
som KID MOD11; punktum og mellomrom normaliseres bort). En post hvis
leverandør mangler gyldig kontonummer avvises høyt ved
listeopprettelse — og flagges i portalens betalbare liste.

## pain.001-filen

`regnmed-core::pain001`: hand-rolled deterministisk XML
(CustomerCreditTransferInitiation, pain.001.001.03 — versjonen norske
banker tar imot), validert mot det offisielle skjemaet (vendored i
docs/pain001/) i tester og CI. KID rir som strukturert
kreditorreferanse (SCOR); fri melding som ustrukturert. Beløp er
heltall øre frem til den avsluttende to-desimalsformatteringen;
EndToEndId per linje er kjøringsradens id, så kontoutskriftens debet
kan spores tilbake til posten den betalte.

Scope (v1): innenlands NOK til norske kontonumre (BBAN).
Utland/IBAN + BIC, filutvekslingsavtaler og direkte bankinnsending
(PSD2) er senere tiere — samme fil.

## Endpoints

- `GET  /companies/{id}/payments/payable` — åpne leverandørposter m/
  kontonummerstatus og «i kjøring»-flagg
- `GET/POST /companies/{id}/payments/runs`
- `POST /companies/{id}/payments/runs/{rid}/approve` → fil + hash
- `GET  /companies/{id}/payments/runs/{rid}/file` (hash-sjekket)
- `POST /companies/{id}/payments/runs/{rid}/settle` — bilag + matcher
- `POST /companies/{id}/payments/runs/{rid}/cancel` — utkast only

Lesing er åpen for alle tilgangsnivåer; alt annet krever bokforing.
Portal: Bank-seksjonen har Betalingsliste-kortet (betalbare poster,
lag liste, godkjenn, last ned pain.001, registrer utbetalt);
leverandørens kontonummer settes på partssiden under Reskontro.

## Where it is tested

- `regnmed-core/src/pain001.rs` — kontonummer MOD11 (normalisering,
  feil kontrollsiffer), deterministisk rendering (KID/melding,
  escaping, CtrlSum), XSD-validering mot vendored skjema.
- `crates/regnmed-api/tests/payments.rs` — end to end: betalbare
  poster m/ manglende-konto-flagg, avvist liste uten konto, kopierte
  kreditordata, fil KUN etter separat godkjenning (KID som SCOR,
  normalisert kontonummer, CtrlSum), DB-immutabilitet for kjøring og
  rader, oppgjør som lukker reskontroen eksakt i én transaksjon,
  annullering avvist etter utbetaling, kjedeverifikasjon.
