# Lønn — første del

**Dette er ikke en komplett lønnsmodul, og skal ikke brukes som om det
var det.** #46 er den største bevisst utsatte delen av ROADMAP, og denne
leveransen tar bare det som kan gjøres riktig i dag. Hva som mangler står
lenger ned, uten pynt.

Det som virker: fastlønn, timelønn fra timeføringen, prosenttrekk fra
skattekortet, arbeidsgiveravgift per sone, feriepengeavsetning med
avgiften på den, og lønnsslipp — bokført som ett ordinært bilag i én
transaksjon.

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
| 5405 Aga av påløpne feriepenger | debet | endring i avsetningen |
| 2780 Påløpt aga av feriepenger | kredit | endring i avsetningen |

Hver linje utelates når den er null. En måned med bare ferieavvikling —
feriepenger utbetales, ingen ordinær lønn — har verken lønnskostnad
eller forskuddstrekk, og bilagsvalideringen avviser nullinjer. Uten
utelatelsen kunne den måneden ikke kjøres i det hele tatt.

Netto går til **2930**, ikke til bank: selve utbetalingen er
betalingslistens jobb (docs/betaling.md), så lønnskjøring og utbetaling
er to atskilte, sporbare handlinger.

**Utbetalte feriepenger er ikke en ny kostnad.** De trekker ned gjelden
som ble avsatt i opptjeningsåret. Bommer man på dette, kostnadsføres
feriepenger to ganger og resultatet blir systematisk for lavt — derfor
har det sin egen test.

## Avgiften på feriepenger man ennå ikke har betalt

Feriepenger opptjent i år utbetales neste år, og arbeidsgiveravgiften på
dem forfaller da. Men *forpliktelsen* oppstår med opptjeningen, så
kostnaden hører hjemme i året den ble opptjent.

Avsetningen er modellert som et **mål, ikke en strøm av tillegg**: etter
enhver kjøring skal påløpt aga være satsen av det som faktisk skyldes, og
kjøringen bokfører differansen. Det er hele forskjellen mellom en
avsetning som holder seg og en som driver:

- **Utbetaling** trekker avsetningen ned av seg selv — gjelden synker, og
  avgiften på det utbetalte ligger allerede i den ordinære aga-linjen.
- **Satsendring** mellom opptjening og utbetaling korrigeres ved neste
  kjøring i stedet for å bli liggende som en uforklarlig rest. Satsen som
  brukes er den gjeldende, med vilje: avgiften skal betales med satsen
  som gjelder når feriepengene utbetales.
- **Gjeld uten avsetning** — opptjent før denne funksjonen fantes, eller
  i en sone uten avgift — tas igjen ved første kjøring, i stedet for å
  bli stående udekket for alltid.

Både skyldige feriepenger og allerede avsatt avgift **utledes per
ansatt** av de innsettings-bare lønnslinjene. Ingenting lagres som en
egen sannhet, så de kan ikke komme i utakt med det som er bokført.

**Negativ gjeld gir ingen avsetning.** Betales det ut mer enn
lønnshistorikken har avsatt, stammer gjelden et annet sted fra; negativ
arbeidsgiveravgift finnes ikke, og å bokføre den ville gjort et hull i
regnskapet om til en inntekt.

**Kontonumrene er valgt, ikke funnet.** Skatteetatens kodeliste navngir
5400 og 2770; nærmeste-nabo-oppslaget i SAF-T-eksporten legger 5405 på
5400 (Arbeidsgiveravgift) og 2780 på 2770 (Skyldig arbeidsgiveravgift).
2785 ville havnet på 2790 «Andre offentlige avgifter» i stedet — derfor
2780.

### Den ærlige begrensningen

Feriepengegjeld som **ikke** stammer fra lønnskjøringene — en
åpningsbalanse, en SAF-T-import, et manuelt bilag — kan ikke knyttes til
noen ansatt, og får derfor ingen avgiftsavsetning. Kjøringen sammenligner
saldoen på 2940 med det lønnshistorikken forklarer og **sier fra med
beløpet** når de spriker, i stedet for å tie eller å finne på en
fordeling. Den delen må i så fall avsettes manuelt.

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
| POST/DELETE | `/companies/{id}/employees/{eid}/link` (koble/koble fra portalbruker) |
| GET | `/companies/{id}/employees/{eid}/link/history` |
| GET/POST | `/companies/{id}/payroll` |
| GET | `/companies/{id}/payroll/{run}/slip/{employee}` (PDF) |
| GET | `/companies/{id}/payroll/hours/{employee}?ar=&maned=` |

Lesing krever tilgang; registrering og kjøring krever `bokforing` eller
`admin`.

## A-meldingen

`regnmed-core::amelding::render` gjør én lønnskjøring om til én
a-melding, håndrullet og deterministisk som RF-1086, og validert mot
Skatteetatens offisielle XSD v2.3 (vendored i docs/amelding/, med kilde,
versjon og hash i docs/amelding/README.md) i enhetstestene og i CI.
**Den sender ingenting** og er ikke koblet til databasen ennå — se
hullene under.

### XSD-en er det minste, ikke det hele

XSD-en er slapp: hver kode er en fri streng, og nesten alt i et
arbeidsforhold er valgfritt. Det som faktisk avgjør om en melding blir
godtatt, står i tre regneark Skatteetaten publiserer per halvår (R1/R2,
lenket i docs/amelding/README.md): kodelisten, inntektskombinasjonene og
forretningsreglene. Renderen bruker bare koder som står der, og nekter
der forretningsreglene krever et felt modellen ikke har. Et oppdiktet
felt blir godtatt av Skatteetaten og er feil hos NAV — det er verre enn
en melding som aldri sendes.

### Hva modellen allerede har

| A-meldingen | Kilde i regnmed |
| --- | --- |
| `opplysningspliktig/norskIdentifikator` | selskapets orgnr |
| `kalendermaaned` | måneden til `payroll_run.utbetalt_dato` — IKKE lønnsmåneden: a-meldingen rapporterer det som ble utbetalt, i måneden det ble utbetalt. En marskjøring utbetalt 2. april hører i aprilmeldingen |
| `meldingsId` | `payroll_run.id` (sjekket mot XSD-ens tegnsett) |
| `leveringstidspunkt` | tidspunktet meldingen lages, i UTC med `Z` — uten sone leses det som norsk tid |
| `inntektsmottaker/norskIdentifikator` | `employee.fodselsnummer` — leses ÉTT sted, som i RF-1086 |
| `arbeidsforhold/arbeidsforholdId` | `employee.id` (stabil på tvers av måneder, F108) |
| `arbeidsforhold/typeArbeidsforhold` | alltid `ordinaertArbeidsforhold` |
| `arbeidsforhold/startdato`, `sluttdato` | `employee.ansatt_fra`, `ansatt_til` |
| `inntekt` `fastloenn` / `timeloenn` / `feriepenger` | `payroll_line.brutto_ore`, `feriepenger_ore` — kombinasjon 111: `kontantytelse`, utløser aga, inngår i trekkgrunnlaget |
| `forskuddstrekk` (`ordinaert`, negativt, F278-2) | `payroll_line.forskuddstrekk_ore` |
| `sumForskuddstrekkPerLoennsutbetalingsdato` (fra 2026-01, F523) | `payroll_run.utbetalt_dato` + sum trekk |
| `sumArbeidsgiveravgift` | `payroll_run.aga_ore` (ikke avsetningen på feriepenger — den er periodisering, ikke avgift å betale). Må være NØYAKTIG grunnlaget × satsen (F185); ellers nektes meldingen med begge tallene |
| `loennOgGodtgjoerelse`: beregningskode, sone, grunnlag, sats | `generelleNaeringer`; `payroll_run.sone` (I→`1`, Ia→`1a`, …); summen av inntektene (F184); satsen fra satsregisteret på utbetalingsdatoen |

`generelleNaeringer` er ikke en ny antakelse: lønnskjøringen bruker
allerede den ordinære satsen per sone, og det ER den beregningskoden
(F197-tabellen). Et selskap med sektorunntak er feil i kjøringen før det
blir feil i meldingen.

### Arbeidsforholdet (migration 0073)

Feltene forretningsreglene krever om arbeidsforholdet ligger nå på
`employee`, alle valgfrie i registeret og redigerbare som resten av
vilkårene:

| A-meldingen | Kolonne | Regel |
| --- | --- | --- |
| `virksomhet` | `business_unit_orgnumber` | F290: underenheten i Enhetsregisteret, MOD11 og ALDRI selskapets eget orgnr |
| `yrke` | `occupation_code` | F115B: 7 siffer (STYRK-08) — bare formatet sjekkes, katalogen er ikke vendored |
| `stillingsprosent` | `position_percent_bp` | F291: 0–100 %, i basispunkter |
| `antallTimerPerUkeSomEnFullStillingTilsvarer` | `full_time_hours_per_week_hundredths` | F291, i hundredeler |
| `arbeidstidsordning` | `working_hours_scheme` | F291: kodelisten i `regnmed-core::amelding` |
| `formForAnsettelse` | `employment_form` | F291: kodelisten i `regnmed-core::amelding` |
| `sisteLoennsendringsdato` | `last_salary_change` | F291 |

Kodelistene sjekkes i applikasjonen, ikke med en check-constraint: de
endres per halvår (R1/R2), og en migrasjon per kodelisteendring ville
vært feil sted. API-et serverer listene fra den ene kopien renderen
sjekker mot (`koder` på `GET …/employees`).

`Employee::amelding_missing()` sier hva a-meldingen ville nektet for, med
**de samme navnene som renderens feilmeldinger** — en enhetstest i
`amelding` fester at `Employment::missing()` og `render` nevner samme
felt. Portalen viser det som «mangler N» per ansatt, med et skjema
(`PUT …/employees/{eid}/employment`, `LONN_SKRIV`) som erstatter alle
sju feltene, så et felt kan tømmes igjen. Ansatte registrert før 0073
har ingen av dem, og ingenting fylles inn for dem: det som mangler,
mangler synlig.

Virksomheten ligger per ANSATT fordi det er der den hører hjemme — et
selskap med to underenheter har ansatte på begge. Renderen tar i dag ÉN
virksomhet per melding; koblingen fra databasen må derfor nekte en
kjøring der de ansatte står på ulike underenheter, til renderen grupperer
per virksomhet (inkludert avgiften, som da må fordeles).

**Fastlønn eller timelønn** lagres nå per lønnslinje
(`payroll_line.pay_kind`), bestemt i kjøringen der det er kjent:
timelister → `hourly`, månedslønnen → `fixed`, et oppgitt beløp →
den ansattes ENE lønnsform; har den ansatte begge eller ingen, lagres
NULL, og meldingen nekter heller enn å gjette. Linjer fra før 0073 er
NULL.

### Hva modellen fortsatt IKKE har — og renderen nekter for

1. **Hele kroner.** Forskuddstrekk og summene å betale er heltall i
   skjemaet, mens lønnskjøringen regner trekk og aga i øre. Renderen
   runder IKKE — da ville meldingen sprike fra det som er bokført på
   2600/2770. Avrundingsregelen hører hjemme i
   `payroll::calculate`/`employer_contribution` med kilde, og må
   verifiseres før den skrives. Inntil da nektes nesten enhver ekte
   kjøring på dette punktet alene.

Renderen nekter også, med eget navn:

- **Avgift som ikke er grunnlag × sats** (F185). Lønnskjøringen runder
  aga per ansatt; Skatteetaten regner på summen. Spriker de, sier
  feilmeldingen begge tallene i stedet for at meldingen sendes med en
  avgift den selv motsier.
- **Negative beløp** — trekk og inntekt har faste fortegn (F277,
  F278-2). En korreksjon er et reverserende bilag og en ny kjøring i
  hovedboken; hvordan den skal se ut i a-meldingen (erstatningsmelding)
  er ikke modellert.

Og ett hull som ikke er et felt, men et oppslag:

2. **Arbeidsforhold uten lønn.** Et aktivt arbeidsforhold skal
    rapporteres hver måned til sluttdato er meldt (F108), også en måned
    uten utbetaling. En lønnskjøring har bare linjer for dem som fikk
    lønn.

Renderen er heller ikke koblet til databasen ennå: ingen kode bygger en
`Melding` av en lagret kjøring. Det er neste steg, sammen med punkt 1.

### Formatet på vei inn

Skatteetatens nye API-er (REST og filopplasting, scope
`skatteetaten:innrapporteringamelding`, systembruker) tar **JSON** etter
OpenAPI-skjemaet `Amelding` (`innrapportering-amelding-api` 0.1.2 på
SwaggerHub), ikke XML. Modellen er den samme felt for felt — `leveranse`,
`oppgave`, `inntektsmottaker`, `arbeidsforhold` med samme navn og samme
påkrevde felt — så XSD-en er fortsatt fasiten for innholdet, og XML-en
kan lastes opp manuelt i Skatteetatens testmiljø. Men innsendingssteget
må serialisere samme tre som JSON. Det bygges sammen med klienten, når
systembrukeren finnes.

## Hva som IKKE er bygget

Rekkefølgen er omtrent den de bør tas i.

1. **A-meldingen.** Månedlig, frist den 5. Renderen finnes og er
   validert mot XSD-en (se «A-meldingen» under), og arbeidsforholdets
   felt finnes nå (0073), men den er ikke koblet til databasen, nekter
   øre der skjemaet vil ha hele kroner, og innsending venter på
   Altinn-systembrukeren (#180, docs/gov.md). Inntil videre er dette
   internt regnskap, ikke rapportering — **selskapet må fortsatt levere
   a-melding på annet vis.** Dette er den viktigste mangelen.
2. **Skattekort fra Skatteetatens API.** I dag registreres trekket
   manuelt. Samme Maskinporten-avhengighet.
3. **Tabelltrekk** (se over).
4. **Sykepengerefusjon, naturalytelser, pensjonstrekk, tariff-logikk,
   OTP** — uttrykkelig utenfor v1 i #46 selv.

## Timelønn fra timeføringen

En lønnslinje kan hente beløpet fra timeføringen i stedet for å bruke
månedslønn: minutter ført i måneden × den ansattes timesats. **Alle**
førte timer teller, fakturerbare eller ikke — arbeidsgiver skylder lønn
for utført arbeid uansett om en kunde faktureres for det.

Regnestykket ligger i `regnmed-core::payroll::hourly_pay` og går fra minutter
direkte, uten et mellomsteg om desimaltimer som bare ville lagt til en
avrunding. Én divisjon, halvt vekk fra null.

### Måneden må være låst

Dette er den viktigste regelen her, og den er en hard forutsetning i
`kjor_lonn`, ikke bare et råd i portalen:

> timelisten for 03/2026 er ikke låst — lås måneden før timelønn
> utbetales, ellers kan timene endres etter at lønnen er bokført

En lønnskjøring er innsettings-bar. Endres timene etterpå, spriker
timelisten og lønnen for alltid, uten noen måte å avstemme dem på.
Månedslåsen i docs/timer.md finnes nettopp for denne rekkefølgen: lås
for lønn, så fakturer.

### Ansatt og portalbruker er to ting

`time_entry` føres av en **person** (en som logger inn), mens `employee`
er lønnsmottakeren identifisert ved fødselsnummer, fordi det er slik
a-meldingen rapporterer. De skal fortsette å være atskilte: en ansatt
trenger ikke portaltilgang, og en portalbruker er ikke nødvendigvis
ansatt.

Migration 0036 legger derfor til en **valgfri, eksplisitt** kobling
(`employee.person_id`), satt av en admin. Den gjettes ikke ut fra navn —
det ville koblet feil person til feil lønn første gang to ansatte het
det samme. Mangler koblingen, sier `hour_basis` fra i stedet for å
returnere null timer.

### Hvordan koblingen settes (migration 0050)

Koblingen avgjør hvem som kan lese en lønnsslipp, så veiene inn er få
og kontrollerte, og hver endring står i det innsettings-bare sporet
`employee_link_change` (hvem koblet hvem, når, av hvem — spørsmålet som
teller når en slipp var lesbar for feil person).

1. **Invitasjonen bærer den ansatte** (hovedveien). Admin oppgir den
   ansattes e-postadresse; invitasjonen (docs/auth.md §7) får en
   `employee_id`, og innløsningen i `/me` setter koblingen i SAMME
   transaksjon som medlemskapet. Da er det den ansatte selv som kobler
   seg, ved å logge inn med sin egen adresse — admin velger aldri en
   person fra en liste, og feilkobling krever at noen andre disponerer
   postkassen. Bare én åpen invitasjon kan love en ansatt om gangen.
   Kan koblingen ikke settes ved innløsning (personen er alt en annen
   ansatt her), består medlemskapet og den ansatte forblir SYNLIG
   ukoblet — en invitasjonstabbe skal aldri knekke en innlogging, og
   aldri skjules bak en stille feilkobling.
2. **Manuell kobling til et eksisterende medlem** (unntaksveien).
   `POST …/employees/{eid}/link` — portalvelgeren viser bare selskapets
   direkte, aktive medlemmer, og vaktene holder uansett hva UI-et
   sender: bare ukoblede ansatte (omkobling er et to-stegs bevisst
   valg: koble fra først), bare mennesker (en integrasjon med
   lønnsslipptilgang ville vært en lekkasje ved konstruksjon), og én
   ansatt per person (delvis unik indeks fra 0036).

`GET /companies/{id}/payroll/hours/{employee}?ar=&maned=` gir minutter,
sats, beløp og `laast`, så portalen bare tilbyr timelønn når det faktisk
er trygt å kjøre.

## Lønnsslipp

`regnmed-core::payslip` rendrer slippen med den samme hand-rolled
PDF-skriveren som fakturaen — samme begrunnelse: et dokument vi står
ansvarlig for, uten en motor med skjult oppførsel.

**Slippen lagres ikke.** Til forskjell fra fakturaen, der PDF-en *er*
salgsdokumentet og derfor bokføres som vedlegg, er lønnsslippen utledet
av lønnslinjen — og linjene er innsettings-bare. Samme linje gir samme
bytes for alltid, så den rendres på forespørsel i stedet for å lagre
enda en kopi av personopplysninger.

Slippen **forklarer** trekket i stedet for bare å oppgi det: grunnlaget
står i parentes («Forskuddstrekk (35 % av 55 000,00)»), og både
feriepengenes trekkfrihet og desembers halve trekk får sin egen linje
når de gjelder. Feriepenger opptjent denne måneden står under en egen
overskrift, tydelig utenfor «Til utbetaling», og hittil-i-år-tallene
står nederst.

`GET /companies/{id}/payroll/{run}/slip/{employee}` → `application/pdf`.
Fødselsdato, ikke fødselsnummer — også på et dokument som sendes til
den ansatte selv.

**Hvem får se den (#55).** Sin egen slipp får enhver ansatt
(`LONNSSLIPP_LES_EGEN`). Andres krever `LONNSSLIPP_LES_ALLE`, som
`bokforing`, `admin` og **revisor** har — men ikke en intern leser.
Fram til #55 lå den i lesebunten, så enhver med lesetilgang kunne laste
ned hvem som helst sin; det var ikke et valg, men en arv fra at lønn kom
sist og gjenbrukte den generelle lesetilgangen. Ber man om en annens
slipp uten retten, svarer endepunktet **404** og ikke 403: den som ikke
får se noe skal heller ikke få vite at det finnes.

Det samme gjelder ansattregisteret (`LONN_LES`): fødselsdato,
månedslønn og trekkprosent er ikke allmenn lesning.

## Portal

Lønn-seksjonen har to kort: **Ansatte** (register + nyregistrering) og
**Lønnskjøring** (historikk + kjøreskjema med én rad per aktiv ansatt,
der brutto kan overstyres og feriepenger legges inn per person).
Ansatte-kortet har en **Portalbruker**-kolonne (koblet / invitert /
ikke koblet) med invitasjons- og koblingsflyten fra 0050; skjemaet for
ny ansatt tar e-postadresse rett i registreringen, så invitasjonen går
ut i samme håndvending.
Hver kjørt måned har en knapp per ansatt som laster ned lønnsslippen.
Ansatte med timesats har en «fra timer»-avkrysning i kjøreskjemaet;
serveren nekter hvis måneden ikke er låst.

Måneder som allerede er kjørt er **deaktivert** i månedsvelgeren, så den
vanligste feilen ikke engang kan forsøkes. Advarselen om at a-meldingen
ikke leveres herfra står i selve kortet, ikke bare i denne filen.

Verifisert i nettleser: registeret viser fødselsdato og ikke
fødselsnummer, juni (allerede kjørt) er deaktivert, og en kjøring
utført gjennom UI-et ga et bilag som summerer til nøyaktig null.

## Testet

- `regnmed-core::payroll` — prosenttrekk, trekkfrie feriepenger, halv skatt
  i desember, frikort, avrunding halvt vekk fra null, og at tabelltrekk
  og sone Ia nektes med begrunnelse.
- Aga-avsetningen: at målet er satsen av gjelden, at livsløpet
  (avsetning → utbetaling) summerer til nøyaktig null, at en satsendring
  korrigeres i sin helhet ved neste kjøring, og at negativ gjeld gir
  null avsetning framfor negativ avgift.
- Mot ekte Postgres: at avsetningen bokføres på 5405/2780 og bilaget
  fortsatt balanserer, at den føres tilbake i sin helhet ved
  ferieutbetaling og etterlater begge kontoene på null, at gjeld
  opptjent uten avsetning tas igjen ved neste kjøring, at saldoen på
  2780 etter hver kjøring er satsen av **hver ansatts** gjeld (fasiten
  bygges per ansatt — avrundingen skjer der), og at feriepengegjeld
  lønnshistorikken ikke forklarer gir en advarsel med beløpet i.
- Koblingen (`tests/grupper/ansattkobling.rs`): at invitasjonen kobler i
  samme transaksjon som medlemskapet med `kilde='invitasjon'` i sporet,
  at en konfliktkobling aldri knekker innloggingen men etterlater den
  ansatte synlig ukoblet, at bare én åpen invitasjon kan love en
  ansatt, og at manuell kobling nekter omkobling uten frakobling,
  person som alt er en annen ansatt, og maskinidentiteter — med begge
  hendelsene navngitt i historikken.
- `regnmed-db/tests/payroll.rs` — mot ekte Postgres: at bilaget balanserer
  eksakt, at hver konto får riktig beløp, at utbetalte feriepenger
  reduserer gjelden i stedet for å bli kostnad, at sone V er nullsats,
  at samme måned ikke kan kjøres to ganger, at kjøringer og identitet
  ikke kan endres i ettertid, og at listen viser fødselsdato og ikke
  fødselsnummer.
- Timelønn: at beløpet regnes fra minutter, at en ULÅST måned nektes,
  at kjøringen går etter låsing og gir riktig brutto og trekk, og at en
  ansatt uten portalbrukerkobling gir en tydelig feil framfor null timer.
- `regnmed-core::amelding` — en månedsmelding validert mot den
  offisielle XSD-en med xmllint (sett feile på et feilstavet element),
  også en 2025-melding med én trekksum og en erstatningsmelding; at
  trekket er negativt per mottaker og positivt per utbetalingsdato, at
  avgiftsgrunnlaget er summen av inntektene; at hvert av de seks
  manglende arbeidsforholdsfeltene og virksomheten nektes med navn; at
  øre der skjemaet vil ha kroner nektes framfor å rundes; og at koder
  utenfor kodelisten, ugyldig fødselsnummer og orgnr nektes. Etter
  review: at kalendermåneden følger utbetalingsdatoen (også over
  årsskiftet, der trekkformatet skifter), at avgift som ikke er grunnlag
  × sats nektes, at negative beløp nektes, at meldingsId, kildesystem og
  år utenfor XSD-ens grenser nektes, at leveringstidspunktet har `Z`, og
  en melding med to mottakere der den ene har frikort.
- `regnmed-core::payslip` — velformet og deterministisk PDF, at
  slippen forklarer trekkgrunnlaget, feriepengenes trekkfrihet og
  desembers halve trekk, at frikort sier frikort, og at
  fødselsnummeret ikke er i dokumentet.
- Lønnsslippen bygget fra en ekte kjøring, med hittil-i-år summert over
  flere måneder — og hentet som `application/pdf` fra en kjørende
  server (2,8 kB, uten fødselsnummer).
