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
(`regnmed-core::pain001::is_valid_account_number` — samme sykliske vekter
som KID MOD11; punktum og mellomrom normaliseres bort). En post hvis
leverandør mangler gyldig kontonummer avvises høyt ved
listeopprettelse — og flagges i portalens betalbare liste.

## Forfall og KID på leverandørposter

En leverandørfaktura bokføres fra innboksen som et ordinært bilag. Dens
forfallsdato og KID ble lest (EHF, bilagstolkning) og så glemt, så
betalingslisten betalte uten KID — leverandøren måtte avstemme for
hånd — og ingenting kunne si hva som forfaller når.

De ligger nå i `payable_terms` (migrasjon 0082) ved siden av
leverandørposten: **opplysninger lest av leverandørens dokument, ikke
regnskapsinnhold**. Hasjkjeden og `entry`-radene røres ikke.

- **Ved bokføring:** `POST …/inbox/{doc}/bokfor` tar `due_date` og `kid`
  og skriver dem i SAMME transaksjon som bilaget, på hver
  leverandørlinje i bilaget. Et bilag uten leverandørlinje avvises
  med opplysningene — de ville blitt lagret og aldri lest — og
  ingenting bokføres.
- **Rette eller føre senere:** `POST …/reskontro/entries/{entry}/terms`
  (`RESKONTRO_SKRIV`) for en post bokført før dette fantes, eller en
  skrivefeil. KID kontrolleres (MOD10/MOD11) før noe lagres.
- **Innsetting, aldri endring:** en retting er en NY rad; den nyeste
  gjelder, og de tidligere står igjen som sporet av hvem som sa hva,
  når. Databasen nekter UPDATE, DELETE og TRUNCATE på tabellen uansett
  rolle (trigger), og appens rolle har bare SELECT og INSERT.
- **Betalingslisten** bruker postens KID når listen ikke oppgir en
  selv. De betalbare postene sorteres med det som forfaller først.
- **Forfalt** for en leverandør (`overdue_ore` i partslisten) er det
  som står igjen på poster med passert forfallsdato. En post uten
  forfallsdato regnes aldri som forfalt: systemet gjetter ikke en
  betalingsbetingelse.

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
  kontonummerstatus, «i kjøring»-flagg, `due_date` og `kid`, det som
  forfaller først øverst
- `POST /companies/{id}/reskontro/entries/{entry}/terms` — før eller
  rett forfall og KID på en leverandørpost (ny rad, nyeste gjelder)
- `GET/POST /companies/{id}/payments/runs`
- `POST /companies/{id}/payments/runs/{rid}/approve` → fil + hash
- `GET  /companies/{id}/payments/runs/{rid}/file` (hash-sjekket)
- `POST /companies/{id}/payments/runs/{rid}/settle` — bilag + matcher
- `POST /companies/{id}/payments/runs/{rid}/cancel` — utkast only

Lesing er åpen for alle tilgangsnivåer; alt annet krever bokforing.
Portal: Bank-seksjonen har Betalingsliste-kortet (betalbare poster,
lag liste, godkjenn, last ned pain.001, registrer utbetalt).
Leverandørsiden under Leverandører viser skyldig og forfalt, de åpne
postene med forfall og KID, og legger valgte poster i en betalingsliste
som så godkjennes av en annen under Bank; kontonummeret settes der.

## Where it is tested

- `regnmed-core/src/pain001.rs` — kontonummer MOD11 (normalisering,
  feil kontrollsiffer), deterministisk rendering (KID/melding,
  escaping, CtrlSum), XSD-validering mot vendored skjema.
- `crates/regnmed-api/tests/grupper/payments.rs` — end to end: betalbare
  poster m/ manglende-konto-flagg, avvist liste uten konto, kopierte
  kreditordata, fil KUN etter separat godkjenning (KID som SCOR,
  normalisert kontonummer, CtrlSum), DB-immutabilitet for kjøring og
  rader, oppgjør som lukker reskontroen eksakt i én transaksjon,
  annullering avvist etter utbetaling, kjedeverifikasjon. Forfall og
  KID: en forfallsdato setter posten først i listen og gjør leverandøren
  forfalt med akkurat den posten, ugyldig KID og fremmed post avvises,
  en post med lagret KID betales MED den (nyeste rad gjelder, rettingen
  står igjen), og tabellen nekter UPDATE i databasen.
- `crates/regnmed-api/tests/grupper/innboks.rs` — forfall og KID på et
  bilag uten leverandørlinje avvises, og dokumentet står fortsatt som
  ubehandlet.
