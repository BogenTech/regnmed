# Lønn — første del

**Dette er ikke en komplett lønnsmodul, og skal ikke brukes som om det
var det.** #46 er den største bevisst utsatte delen av ROADMAP, og denne
leveransen tar bare det som kan gjøres riktig i dag. Hva som mangler står
lenger ned, uten pynt.

Det som virker: fastlønn, prosenttrekk fra skattekortet,
arbeidsgiveravgift per sone, feriepengeavsetning — bokført som ett
ordinært bilag i én transaksjon.

## En lønnskjøring er ett bilag

Ingen parallell sannhet: linjene lagres for lønnsslipp og senere
a-melding, men tallene i hovedboken *er* bilaget.

| Konto | | Beløp |
| --- | --- | --- |
| 5000 Lønn til ansatte | debet | ordinær bruttolønn |
| 2600 Forskuddstrekk | kredit | forskuddstrekk |
| 2930 Skyldig lønn | kredit | netto til utbetaling |
| 2940 Skyldige feriepenger | debet | feriepenger utbetalt denne måneden |
| 5090 Feriepenger | debet | månedens avsetning |
| 2940 Skyldige feriepenger | kredit | månedens avsetning |
| 5400 Arbeidsgiveravgift | debet | aga |
| 2770 Skyldig arbeidsgiveravgift | kredit | aga |

Netto går til **2930**, ikke til bank: selve utbetalingen er
betalingslistens jobb (docs/betaling.md), så lønnskjøring og utbetaling
er to atskilte, sporbare handlinger.

**Utbetalte feriepenger er ikke en ny kostnad.** De trekker ned gjelden
som ble avsatt i opptjeningsåret. Bommer man på dette, kostnadsføres
feriepenger to ganger og resultatet blir systematisk for lavt — derfor
har det sin egen test.

Kjøringer og ansattregister er innsettings-bare (trigger + kolonne-
rettigheter). Samme måned kan ikke kjøres to ganger; en korreksjon er et
reverserende bilag og en ny kjøring, som ellers i hovedboken.

## Forskuddstrekk: to regler som ser ut som gaver

Begge er bare tidfesting. Skattekortprosenten er beregnet over 10,5
måneder nettopp for at de skal gå opp, så det er *riktig* å anvende dem
— ikke dobbelttelling.

- **Feriepenger er trekkfrie** i utbetalingsåret.
- **Halv skatt i desember** (skattebetalingsloven). Siden 2016 kan
  arbeidsgiver i stedet ta det i annen halvdel av november; regnmed
  gjør desember, det vanlige valget, og sier det her.

## Satsene er data, ikke kode

Alt ligger i satsregisteret (docs/regelverk.md) med kilde per rad.

| Domene | Verdi | Kilde |
| --- | --- | --- |
| `aga_sone_i` | 14,1 % | Skattedirektoratets melding, aga til folketrygden for 2026 |
| `aga_sone_ii` | 10,6 % | ↑ |
| `aga_sone_iii` | 6,4 % | ↑ |
| `aga_sone_iv` | 5,1 % | ↑ |
| `aga_sone_iva` | 7,9 % | ↑ |
| `aga_sone_v` | 0 % | ↑ — nullsats er en sats, ikke en manglende verdi |
| `feriepenger_lovens_minimum` | 10,2 % | ferieloven §10 nr. 2 |
| `feriepenger_over_60` | 12,5 % | ferieloven §10 nr. 3 (+2,3 prosentpoeng) |

Meldingen sier selv at verken soneinndeling eller satser er endret fra
2025 til 2026, så periodene starter 2025-01-01 — den tidligste
**verifiserte** datoen, ikke gjettet historikk.

Feriepengesatsen ligger per ansatt, fordi den er et faktum om
arbeidsforholdet (alder, tariff), ikke en systeminnstilling.

**Den ekstra arbeidsgiveravgiften på 5 % over 750 000 kroner ble fjernet
fra 2025.** Den finnes ikke i registeret fordi den ikke finnes.

## To ting vi nekter å gjøre

Begge følger samme doktrine som RF-1086-kodene (docs/aksjonaer.md): der
vi ikke kan regne riktig, sier vi fra i stedet for å tilnærme.

### Tabelltrekk

Trekktabellene er Skatteetatens datafiler. Uten dem finnes det ingen
forsvarlig måte å regne tabelltrekk på, og en tilnærming ville blitt den
ansattes restskatt. En kjøring med `trekk_type = 'tabell'` stopper:

> tabelltrekk (tabell 7100) er ikke støttet: trekktabellene er
> Skatteetatens datafiler, og regnmed tilnærmer dem ikke

Skattekortet kan i praksis alltid leses som prosenttrekk inntil videre.

### Sone Ia

Den reduserte satsen på 10,6 % gjelder bare til **fribeløpet** er brukt
opp — 850 000 kroner i avgiftsbesparelse per år, altså rundt 24,3
millioner i lønn. Fribeløpet er bagatellmessig støtte og forbrukes også
av ting regnmed ikke ser. Å regne 10,6 % flatt ville underrapportert
avgift til Skatteetaten, så sone Ia avvises — **før** satsoppslaget, slik
at feilmeldingen ikke sier «legg inn satsen», som er stikk motsatt av
riktig råd.

## Personvern

Ansattlisten bærer **fødselsdato**, ikke fødselsnummer — samme valg som i
aksjeeierboken. Nummeret lagres fordi a-meldingen vil kreve det, men
leses bare der det trengs. Testene fester det: nummeret skal ikke være i
listingen.

## Endepunkter

| Metode | Sti |
| --- | --- |
| GET/POST | `/companies/{id}/employees` |
| GET/POST | `/companies/{id}/payroll` |

Lesing krever tilgang; registrering og kjøring krever `bokforing` eller
`admin`.

## Hva som IKKE er bygget

Rekkefølgen er omtrent den de bør tas i.

1. **A-meldingen.** Månedlig, frist den 5., via Altinn. Krever
   Maskinporten-scope vi ikke har (docs/gov.md). Uten den er dette
   internt regnskap, ikke rapportering — **selskapet må fortsatt levere
   a-melding på annet vis.** Dette er den viktigste mangelen.
2. **Skattekort fra Skatteetatens API.** I dag registreres trekket
   manuelt. Samme Maskinporten-avhengighet.
3. **Tabelltrekk** (se over).
4. **Lønnsslipp** som dokument til den ansatte. Linjene ligger klare;
   det mangler en `regnmed-core::pdf`-layout, som faktura har.
5. **Aga-avsetning på ikke-utbetalte feriepenger.** Avgiften beregnes i
   dag på det som faktisk utbetales — som er når den forfaller. Den
   regnskapsmessige avsetningen på avsatte feriepenger krever en
   matchende nedtrekk ved utbetaling; en halvbygd avsetning er verre enn
   ingen, så feltet finnes og står på null.
6. **Timelønn i kjøringen.** Feltet finnes på den ansatte, men en kjøring
   tar beløp per linje — timer × sats regnes ikke automatisk fra
   timeføringen (docs/timer.md) ennå.
7. **Sykepengerefusjon, naturalytelser, pensjonstrekk, tariff-logikk,
   OTP** — uttrykkelig utenfor v1 i #46 selv.
8. **Portal-seksjon.** Endepunktene finnes; UI-et kommer i neste del.

## Testet

- `regnmed-core::lonn` — prosenttrekk, trekkfrie feriepenger, halv skatt
  i desember, frikort, avrunding halvt vekk fra null, og at tabelltrekk
  og sone Ia nektes med begrunnelse.
- `regnmed-db/tests/lonn.rs` — mot ekte Postgres: at bilaget balanserer
  eksakt, at hver konto får riktig beløp, at utbetalte feriepenger
  reduserer gjelden i stedet for å bli kostnad, at sone V er nullsats,
  at samme måned ikke kan kjøres to ganger, at kjøringer og identitet
  ikke kan endres i ettertid, og at listen viser fødselsdato og ikke
  fødselsnummer.
