# Periodisering

Fordeling av kostnad og inntekt over månedene de hører hjemme i —
**rskl. §4-1 nr. 2 og 3**, opptjeningsprinsippet og
sammenstillingsprinsippet. Husleie betalt for et helt år, forsikrings-
premier, årsabonnementer og opptjent-men-ikke-fakturert arbeid skal ikke
ligge som en pukkel i betalingsmåneden: da viser månedsrapportene,
nøkkeltallene og avviksrapporten mot budsjettet noe som ikke er drift.

## 1. Regelen som ikke må brytes

**Periodisering flytter kostnad og inntekt, ALDRI merverdiavgift.**

Tidfestingen av avgiften følger salgsdokumentet (mval. §15-9): en
husleie betalt for hele 2026 er fradragsberettiget i sin helhet i
terminen fakturaen hører hjemme i, uansett hvordan kostnaden fordeles i
resultatet. Fordeler man avgiften med, blir mva-meldingen feil — og
feilen er **stille**, fordi resultatet ser riktigere ut etterpå.

Derfor:

- `PeriodiseringDraft.total_ore` er et **nettobeløp**; kalleren har alt
  skilt avgiften ut.
- Linjene kjøringen bokfører bærer **ingen `vat_code`**.
- Planen har ingen mva-kolonne. Avgiften finnes på KILDEBILAGET, som er
  ført på vanlig måte med hele beløpet.

Dette står både i `regnmed-core::periodisering`, i
`regnmed-db::periodisering` og i migrasjonen, fordi det er den ene
tingen som blir gjort feil hvis den ikke står noe sted.

## 2. Modellen

| | |
| --- | --- |
| `periodisering` | planen: kildebilag, beskrivelse, resultatkonto, balansekonto, nettobeløp, fra- og til-måned, dimensjoner, notat |
| `periodisering_run` | innsettings-bar kjørelogg: én rad per (plan, måned), enten et ført bilag eller en logget feil |

Mønsteret er avskrivningenes (#40) og de repeterende fakturaenes (#30),
som begge er i drift:

- **Én transaksjon per (plan, måned)**: bilag datert månedsslutt +
  kjøringsrad. En delvis unik indeks
  (`periodisering_run_once … where voucher_id is not null`) gjør en måned
  umulig å føre to ganger — idempotensen ligger i databasen, ikke i et
  flagg vi husket å sette.
- **Feil logges med detalj** og stopper ikke kjøringen for de andre.
- **Månedlig CronJob** `regnmed periodiser` (deploy/base, kjører den 1.
  og fører måneden som nettopp endte).

**Retningen ligger i fortegnet.** En forskuddsbetalt KOSTNAD har
positivt totalbeløp (debet resultatkonto, kredit balansekonto hver
måned), en uopptjent INNTEKT negativt. Vi lagrer ikke en «type» ved
siden av fortegnet: to felter som kan motsi hverandre er en feilkilde,
ikke en opplysning.

**Kontoene oppgis av den som oppretter planen** (1700 forskuddsbetalt
kostnad, 2900 forskuddsbetalt inntekt er de vanlige) — vi gjetter aldri
en konto.

## 3. Fordelingen summerer EKSAKT

`regnmed-core::periodisering::manedsbelop` har samme kontrakt som
`anlegg::manedsbelop`: alle månedene unntatt den siste får det avrundede
grunnbeløpet, og den siste tar resten. Et øre oppfunnet her ville havnet
i resultatet og aldri gått opp mot kildebilaget.

10 000,01 kr over 12 måneder blir 83,33 × 11 og 83,38 i desember —
balansekontoen tømmes på øret. Egenskapen er testet uttømmende over
vanskelige totaler og 1..=36 måneder, og sett feile mot en naiv
fordeling.

Inntektsperiodiseringer er negative i hovedbokens fortegn;
heltallsdivisjon i Rust trunkerer mot null, så grunnbeløpet får riktig
fortegn og resten kan ikke drive andre veien. Også testet.

## 4. Livsløpet

- **Redigerbar til første kjøring.** Etter det er planen historikk, og
  bare stoppingen kan settes — håndhevet av en trigger, ikke bare av
  koden: en plan som endres etter at halve beløpet er bokført ville
  gjort at delene ikke lenger summerer til totalen.
- **Stopping er enveis.** Måneder som alt er ført står; de gjenstående
  føres aldri. En plan slettes aldri, fordi bilagene viser til den.

## 5. Web API

| Endepunkt | Rett |
| --- | --- |
| `GET /companies/{id}/periodiseringer` | `BILAG_LES` |
| `POST /companies/{id}/periodiseringer` | `BILAG_BOKFOR` |
| `POST …/periodiseringer/{id}/stopp` | `BILAG_BOKFOR` |
| `POST …/periodiseringer/{id}/kjor` | `BILAG_BOKFOR` |
| `GET …/periodiseringer/{id}/kjoringer` | `BILAG_LES` |

Å opprette en plan er bokføringsarbeid: den avgjør hva som blir bokført
hver måned, og hører derfor på samme side av grensen som posteringen
selv. `kjor` finnes av samme grunn som `assets/depreciate` gjør — å
vente på en CronJob for å se om planen var riktig er en dårlig måte å
jobbe på.

Portalen: **Periodisering**-kortet i Hovedbok-seksjonen (opprett, kjør
nå, stopp, med ført/gjenstående per plan).

## 6. Hvor det er testet

- `regnmed-core::periodisering` — fordelingen, uttømmende.
- `crates/regnmed-db/tests/periodisering.rs` — et helt år fordelt fra et
  ekte kildebilag (balansekontoen tømmes på øret), at samme måned ikke
  kan føres to ganger, at stopping lar det førte stå, og at en påbegynt
  plan ikke kan skrives om (triggeren, ikke bare koden).

## 7. Ikke bygget

- **Automatisk forslag** om periodisering ut fra fakturaens
  leveringsperiode. Bilagstolkningen (#34) foreslår, den bokfører ikke —
  et forslag her ville fulgt samme regel, men er ikke skrevet.
- **Periodisering av en enkelt fakturalinje** i stedet for et beløp:
  planen tar et beløp og to kontoer, og kildebilaget er en referanse.
