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

**Kontoene finnes fra første dag.** Startkontoplanen et selskap får ved
onboarding (`regnmed-db::marketplace`, `STARTER_ACCOUNTS`) har de sju
kontoene over med navnene i tabellen — 5090, 2930 og 2940 står ikke i
Skatteetatens standardkatalog, så brukeren skulle ellers måttet finne på
navn selv. 5405/2780 opprettes av kjøringen første gang de trengs.
**Selskaper onboardet før 2026-11-11 er ikke migrert**: mangler en
konto, nekter kjøringen FØR noe bokføres og navngir hver konto som
mangler («Legg dem til under Hovedbok → Kontoplan»), i stedet for
hovedbokens engelske feilmelding for den første den snubler i.

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

## Tabelltrekk

Tabelltrekket regnes ikke ut av en tabell vi har tastet inn, men av
**Skatteetatens egen trekkrutine**: `github.com/Skatteetaten/trekktabell`
(Java, `trekkrutine2026`) er det programmet Skatteetaten selv lager den
publiserte tabellfila med. `regnmed-core::withholding_table` er en port
av den, og den er **sjekket mot hele den publiserte fila for 2026**
(«trekktabeller i tekstformat», `alleTabelleneIEnFilTilSkatteetatenNo.txt`,
1 063 799 rader, alle tabellnumre og alle sju perioder): hver rad gir
nøyaktig samme trekk. Et deterministisk utvalg (hver 211. rad, 5 043
rader) ligger i `docs/lonn/trekktabell-2026-utvalg.txt` og kjøres i CI;
hele fila kan kjøres lokalt med `TREKKTABELL_FILE=…`.

**Rutinen regner med flyttall, med vilje.** regnmed bruker aldri
flyttall for penger, men her er myndighetens rutine definert i
IEEE-doubles, og den eneste måten å lande på Skatteetatens kroner er å
gjøre de samme operasjonene i samme rekkefølge og runde likt (Javas
`Math.round`). Inn og ut går hele kroner; ingen flyttall når hovedboken.

I lønnskjøringen: månedstabellen, trekkgrunnlaget i hele kroner (øre
under en krone flytter aldri trekket over neste 100-kronerstrinn), og
**halvt tabelltrekk i desember** — som for prosenttrekk, fordi
10,5-månederstabellene er regnet for det. Feriepenger er trekkfrie som
før.

**Tabellene over 12 måneder** (7350, 7500, 7700, 6350, 6500, 6700,
0100, 0101, 7150, 7160) regnes også: rutinen kjenner dem selv, og
kjøringen trekker da fullt i desember og også i feriepengene (se «12
måneder og frikort» under).

Det kjøringen fortsatt **nekter**, med begrunnelse:

- **Et år uten konstanter.** Satsene i trekkrutinen endres hvert år
  (trinnskatt, personfradrag, trygdeavgift); et år som ikke er lagt inn
  stopper kjøringen i stedet for å bruke fjorårets. Nytt år = ny
  `Constants` i `withholding_table` + nytt utvalg fra Skatteetatens fil,
  og det hører til den årlige regelverksrevisjonen.

## Det vi fortsatt nekter å gjøre

Samme doktrine som RF-1086-kodene (docs/aksjonaer.md): der vi ikke kan
regne riktig, sier vi fra i stedet for å tilnærme.

### Sone Ia

Den reduserte satsen på 10,6 % gjelder bare til **fribeløpet** er brukt
opp — 850 000 kroner i avgiftsbesparelse per år, altså rundt 24,3
millioner i lønn. Fribeløpet er bagatellmessig støtte og forbrukes også
av ting regnmed ikke ser. Å regne 10,6 % flatt ville underrapportert
avgift til Skatteetaten, så sone Ia avvises — **før** satsoppslaget, slik
at feilmeldingen ikke sier «legg inn satsen», som er stikk motsatt av
riktig råd.

## Skattekort fra Skatteetaten (migration 0078)

Trekket på den ansatte kommer fra skattekortet, og skattekortet hentes
nå fra Skatteetatens API «Skattekort til arbeidsgiver» i stedet for å
tastes inn (`skatteetaten:skattekorttilarbeidsgiver`, systembruker for
selskapet, samme skinne som a-meldingen — docs/gov.md). En bestilling
(`bestillSkattekort`, `HENT_ALLE_OPPGITTE`) gjelder alle med
fødselsnummer som er ansatt en eller annen gang i inntektsåret; svaret
hentes med referansen (`BR…`). I testmiljøet var svaret klart ved første
forespørsel, tre sekunder etter bestillingen, så portalen venter noen
sekunder og har ellers en «Hent svaret»-knapp.

**Bare for inneværende år.** Den ansatte har ett trekk, og hver
lønnskjøring leser det; et kort for neste år ville blitt brukt på årets
lønn (i desember ville desemberlønnen gått på neste års kort). Neste års
kort hentes derfor i januar, før årets første kjøring, og serveren nekter
et annet år — også når svaret på en eldre bestilling hentes senere.
Avviser Skatteetaten en bestilling eller svaret på den, står grunnen i
listen over bestillinger (bak `LONN_LES`), og et avvist svar er endelig:
det hentes ikke på nytt.

**Alt er innsettings-bart, og forespørselen skrives FØR kallet**
(`tax_card_request`, migration 0079): en bestilling Skatteetaten
avviser — eller som forsvinner underveis — står likevel i registeret over
kall til myndighetene, med feilen i `tax_card_request_failure`. Godtar
Skatteetaten den, blir den en `tax_card_order` med samme id, og hver
ansatts oppføring i svaret lagres ordrett i `tax_card`, med skattekort-id
og utstedelsesdato. Det er beviset for hvorfor trekket var det det var.

**Bare det lønnskjøringen regner riktig settes på den ansatte**, i
samme transaksjon som kortet lagres. Det er
`loennFraHovedarbeidsgiver` som styrer — regnmed betaler ordinær lønn
som hovedarbeidsgiver; pensjon, NAV og biarbeidsgiver ligger i kortet
men brukes ikke.

| Kortet sier | regnmed |
| --- | --- |
| Trekkprosent over 10,5 måneder | **settes** (prosenten leses som desimaltekst, aldri som flyttall) |
| Trekkprosent over 12 måneder (bl.a. kildeskatt) | **settes**, merket 12 måneder (under) |
| Trekktabell | **settes** når tabellnummeret og kortet er enige om 10,5 eller 12 måneder; ellers beholdes |
| Frikort uten beløpsgrense, eller «ikke trekkplikt» | **settes** til ingen trekk |
| Frikort med beløpsgrense | **settes** med grensen arbeidsgiveren er tildelt (under) |
| Trekk over 11 måneder, eller et annet antall | beholdes — regnmed kjenner reglene for 10,5 og 12 |
| Ikke skattekort | beholdes — uten kort skal arbeidsgiver trekke 50 %, og det står i merknaden |

Det som beholdes, får en **merknad** med grunnen, i portalen og i
API-et; den ansatte står med trekket den hadde. **Lønnskjøringen sier
fra** om hver ansatt hvis nyeste kort for året ikke ble satt, med
merknaden — ellers ville kjøringen trukket det gamle trekket uten et ord.
Ingen kort settes i stillhet feil, og ingen kjøring går stille forbi et
kort som ikke ble satt.

Feltverdiene er slik testmiljøet faktisk sendte dem 2026-11-12, lagret i
`docs/lonn/skattekort-svar-test-2026.json` og brukt av testene:
camelCase (`loennFraHovedarbeidsgiver`), mens dokumentasjonens eget
eksempel skriver dem med store bokstaver. Bestillingen er et kall til
Skatteetaten på selskapets vegne, så **kundevilkårene må være godtatt**
først, og den står i registeret over kall til myndighetene
(`/authority-calls`, docs/vilkar-teknisk.md).

Ikke bygget ennå: endringsbestillinger (`HENT_KUN_ENDRING`) og varsel
ved endring — i dag hentes kortet på nytt med knappen — og bestillinger
over 1000 ansatte, som API-et ikke tar i ett kall.

## 12 måneder og frikort (migration 0080)

**Kort over 12 måneder** — bl.a. kildeskatt på lønn — trekker samme
sats hver måned, **også i feriepengene og i lønna før jul**
(skattebetalingsloven § 5-7 annet ledd; Skatteetaten: «Dette gjeld også
i feriepengane og i lønna før jul»). Trekkfritaket i ferietiden og det
halve trekket i desember gjelder bare kort over 10,5 måneder. For
prosentkort lagres det på den ansatte (`withholding_twelve_months`);
for tabeller ligger det i tabellnummeret.

**Frikort med beløpsgrense**: ingen trekk så lenge årets trekkpliktige
utbetalinger fra denne arbeidsgiveren er under grensen arbeidsgiveren er
tildelt, deretter **50 %** av det som overstiger den (Skatteetaten:
«trekke 50 prosent skatt av lønna» over grensen). Det som er utbetalt
tidligere i året, telles fra de lagrede lønnslinjene etter
utbetalingsdato.

**En grense på 0 kr settes ikke**: kortet står med begrunnelsen, og
trekket føres for hånd. Skatteetatens eget testsvar har 0 kr på
hovedarbeidsgiver, biarbeidsgiver og NAV samtidig, så 0 kan ikke der
bety «fordelt til en annen arbeidsgiver» — og en gal tolkning tar
halvparten av hver lønnsslipp. Avklares med Skatteetaten.

**Frikortet må være for utbetalingsåret.** Telleren starter på null
hvert år, så en januarkjøring med fjorårets frikort ville gitt hele
grensen trekkfritt én gang til. Kjøringen nekter derfor en ansatt med
frikort uten et hentet frikort for utbetalingsåret («hent årets
skattekort»). **Lønn utbetalt utenfor regnmed** tidligere i året (et
system selskapet kom fra) telles ikke mot grensen; kjøringen advarer
når en frikortansatt var ansatt før utbetalingsmåneden uten at regnmed
har lønn til vedkommende i året.

**Trekket avrundes ned til hele kroner** for alle metoder: a-meldingen
tar forskuddstrekk i hele kroner, og en kjøring kan ikke gjøres om, så
et trekk med øre ville gitt en måned som aldri kunne leveres. Ned, så
ingen trekkes mer enn kortet sier.

**Arbeidsgiveravgiften regnes ÉN gang per kjøring, på det samlede
grunnlaget, og avrundes ned til hele kroner** — «Ved beregning av
skatte- og avgiftskrav avrundes beløpet ned til nærmeste hele krone»
(skattebetalingsforskriften § 10-4-4). Skatteetatens egen
a-meldingsveiledning gjør det samme: 12 250 kr × 14,1 % = 1 727,25
oppgis som 1 727. Én gang og ikke per ansatt, fordi kravet er
grunnlaget × satsen (F185): avrundet per ansatt kunne summen bomme med
opptil en krone per ansatt, og meldingen ville blitt avvist. Det som
lagres per lønnslinje (`payroll_line.aga_ore`) er den ansattes andel,
avrundet for seg; den leses ikke, og den er ikke kravet —
`payroll_run.aga_ore` er. Avsetningen på feriepenger (5405/2780) er en
periodisering, ikke et krav, og beholder øre.

To punkter kildene ikke avgjør, og der regnmed velger siden som ikke kan
gi den ansatte restskatt: **feriepenger telles med** i grensen
(«all trekkpliktig lønn og godtgjersle»), og trekket over grensen
**halveres ikke i desember**. For mye trekk kommer tilbake i
skatteoppgjøret; for lite blir restskatt.

**Lønnsslippen sier hvordan trekket ble regnet** («35,00 %», «tabell
8000», «frikort, 50 % av … over beløpsgrensen»), lagret med lønnslinjen
(`withholding_method`) så slippen viser det som ble gjort da, ikke det
den ansattes kort sier i dag. Linjer fra før 0080 viser bare
grunnlaget — kortet den ansatte har i dag er ikke nødvendigvis det
linjen ble kjørt med.

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
| GET | `/companies/{id}/tax-cards` (bestillinger og svar) |
| POST | `/companies/{id}/tax-cards?year=` (bestill og hent skattekort) |
| POST | `/companies/{id}/tax-cards/{order}/answer` (hent et svar som ikke var klart) |

Lesing krever tilgang; registrering og kjøring krever `bokforing` eller
`admin`.

## A-meldingen

`regnmed-core::amelding::render` gjør én lønnskjøring om til én
a-melding, håndrullet og deterministisk som RF-1086, og validert mot
Skatteetatens offisielle XSD v2.3 (vendored i docs/amelding/, med kilde,
versjon og hash i docs/amelding/README.md) i enhetstestene og i CI.
**Den sender ingenting** — innsending venter på Altinn-systembrukeren
(#180). Fra en lagret kjøring lages meldingen av
`regnmed-db::payroll::amelding_for_run`, se «Fra lagret kjøring» under.

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
| `sumArbeidsgiveravgift` | `payroll_run.aga_ore` (ikke avsetningen på feriepenger — den er periodisering, ikke avgift å betale). Må være NØYAKTIG grunnlaget × satsen avrundet ned til hele kroner (F185, skattebetalingsforskriften § 10-4-4); ellers nektes meldingen med begge tallene |
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
virksomhet per melding; `amelding_for_run` nekter derfor en kjøring der
de ansatte står på ulike underenheter, til renderen grupperer per
virksomhet (inkludert avgiften, som da må fordeles).

**Fastlønn eller timelønn** lagres nå per lønnslinje
(`payroll_line.pay_kind`), bestemt i kjøringen der det er kjent:
timelister → `hourly`, månedslønnen → `fixed`, et oppgitt beløp →
den ansattes ENE lønnsform; har den ansatte begge eller ingen, lagres
NULL, og meldingen nekter heller enn å gjette. Linjer fra før 0073 er
NULL.

### Hva modellen fortsatt IKKE har — og renderen nekter for

1. **Kjøringer fra før hele kroner.** Skjemaet vil ha trekk og avgift i
   hele kroner. Lønnskjøringen avrunder nå begge ned (se over), men en
   kjøring bokført før det kan bære øre, og kjøringer gjøres ikke om.
   Renderen runder IKKE — da ville meldingen sprike fra det som er
   bokført på 2600/2770 — så slike kjøringer nektes, og meldingen for
   den måneden må leveres på annet vis.

Renderen nekter også, med eget navn:

- **Avgift som ikke er grunnlag × sats** (F185), avrundet ned til hele
  kroner. Spriker bokført avgift fra det, sier feilmeldingen begge
  tallene i stedet for at meldingen sendes med en avgift den selv
  motsier. Det er dette en kjøring fra før avrundingen møter.
- **Negative beløp** — trekk og inntekt har faste fortegn (F277,
  F278-2). En korreksjon er et reverserende bilag og en ny kjøring i
  hovedboken; hvordan den skal se ut i a-meldingen (erstatningsmelding)
  er ikke modellert.

Og ett hull som ikke er et felt, men et oppslag:

2. **Arbeidsforhold uten lønn.** Et aktivt arbeidsforhold skal
    rapporteres hver måned til sluttdato er meldt (F108), også en måned
    uten utbetaling. En lønnskjøring har bare linjer for dem som fikk
    lønn.

### Fra lagret kjøring

`GET /companies/{id}/payroll/{rid}/amelding` (`LONN_KJOR`) bygger
meldingen av en lagret kjøring med `amelding_for_run`. JSON-svaret er en
forhåndsvisning som lister **alle** hindringene samlet, med navn — så
lønnsansvarlig kan rette alt i én runde i stedet for å møte dem én og én
fra renderen. `?format=xml` gir filen, og bare når listen er tom.
Renderens egen dom tas med først når vår liste er tom, ellers sier den
det første punktet en gang til med andre ord.

Hindringene `amelding_for_run` finner:

- arbeidsforhold som mangler felt (samme navn som renderen bruker);
- lønnslinjer uten lagret lønnsform (`pay_kind` NULL — kjøringer fra før
  0073, eller et oppgitt beløp for en ansatt med begge lønnsformer);
- ansatte på ulike virksomheter i samme kjøring;
- ansatte som var ansatt i kjøringens måned, men ikke har noen linje i
  den (punkt 2 over) — en melding uten dem er feil hos NAV, så den
  nektes i stedet for å sendes ufullstendig;
- manglende aga-sats i satsregisteret på utbetalingsdatoen.

Fødselsnummeret leses ÉTT sted, her, som i RF-1086. Derfor krever
endepunktet `LONN_KJOR` og ikke `LONN_LES`: filen bærer alle ansattes
fødselsnummer, som resten av lønns-API-et aldri viser — den som kjører
lønn og leverer meldingen trenger det, en leser (eller revisor) gjør
ikke. Tilgangstesten fester begge sider og er sett feile.

`meldingsId` er kjøringens id; `leveringstidspunkt` er når filen lages.
Erstatningsmelding (`erstatterMeldingsId`) er ikke modellert.

Portalen har en «Sjekk»-knapp per kjøring som viser hindringene, eller
«Last ned XML» når meldingen kan lages.

### Formatet på vei inn

Skatteetatens nye API-er (REST og filopplasting, scope
`skatteetaten:innrapporteringamelding`, systembruker) tar **JSON** etter
OpenAPI-skjemaet `Amelding` (`innrapportering-amelding-api` 0.1.2 på
SwaggerHub), ikke XML. Modellen er den samme felt for felt — `leveranse`,
`oppgave`, `inntektsmottaker`, `arbeidsforhold` med samme navn og samme
påkrevde felt — så XSD-en er fortsatt fasiten for innholdet, og XML-en
kan lastes opp manuelt i Skatteetatens testmiljø.

Derfor bygger renderen meldingen ÉN gang, som et lite typet tre, og
skriver det enten som XML (`render`, validert mot XSD-en) eller som JSON
(`render_json`, det REST-API-et tar). Samme validering, samme felt; bare
typene skiller: lister er lister også med ett element,
`BeloepSomHeltall` er JSON-tall, `Boolsk` er `true`/`false`, og
desimaltall forblir strenger. En test fester at hver verdi i XML-en også
står i JSON-en, så de to formatene ikke kan gli fra hverandre.

Klienten (`regnmed-gov::amelding`, docs/gov.md) sender JSON-en med
kundens systembrukertoken og henter tilbakemeldingen via Dialogporten.
Den er prøvd mot Skatteetatens testmiljø med en melding rendret av
regnmed selv: `mottatt`, ingen avvik.

**To meldinger for samme måned LEGGES SAMMEN.** En ny melding med ny
`meldingsId` for samme kalendermåned kommer i tillegg til den forrige —
periodesummene i tilbakemeldingen ble doblet da testen ble kjørt to
ganger. Derfor: et nytt forsøk på SAMME innsending skal gjenbruke
`idempotencyKey` (da svarer Skatteetaten med første kvittering), og en
rettet melding skal være en erstatningsmelding (`erstatterMeldingsId`),
ikke en ny. Begge deler er nå modellert i innsendingssporet under.

### Innsendingssporet (migration 0074)

**Først kundevilkårene.** Innsendingen nekter til en administrator har
godtatt kundevilkårene for selskapet (docs/vilkar-teknisk.md, «Aksept»):
Digdirs og Skatteetatens vilkår tillater ingen innsending på kundens vegne
uten en avtale med kunden.

**Det som sendes, lagres før det sendes.** `amelding_submission` holder
én rad per innsending: kjøringen, `meldingsId` (ny per innsending — en
erstatning trenger sin egen, så det er IKKE kjøringens id),
`idempotencyKey`, kalendermåneden, `erstatterMeldingsId` og **den
eksakte JSON-kroppen**. Hva som skjedde med den står i
`amelding_submission_event` (`receipt`, `failed`, `feedback`); statusen
— `ikke_levert`, `levert`, `tilbakemelding` — beregnes av hendelsene og
lagres aldri. Begge tabellene er innsettings-bare som hovedboken, og
databasen tillater én kvittering og én tilbakemelding per innsending.

`regnmed-db::amelding_submission::prepare_submission` avgjør hva som
sendes:

- **Finnes en innsending uten kvittering, sendes DEN** — samme kropp,
  samme nøkkel, byte for byte. Et nettverksbrudd eller et 503 fra
  Skatteetaten blir altså aldri en melding til, og feilen står i sporet
  (`siste_feil`).
- **Er kjøringen levert, nektes en ny melding** med beskjed om at den
  ville blitt lagt til. Bare `erstatt: true` lager en ny, og da som
  erstatning for den sist leverte, med ny `meldingsId` og ny nøkkel.
- Ellers rendres en ny fra kjøringen slik den står (`amelding_for_run`,
  med alle hindringene samlet om noe mangler).

To trykk samtidig gir ÉN innsending: kjøringens rad låses
(`for update`) i transaksjonen som avgjør. Testen
`concurrent_sends_create_one_submission` ble sett feile uten låsen.

API: `POST …/payroll/{rid}/amelding/submissions` (`LONN_KJOR`; body
`{"erstatt": bool}`) sender og lagrer kvittering eller feil;
`GET …/submissions` (`LONN_LES`) lister sporet uten kropp;
`POST …/submissions/{sid}/feedback` henter og lagrer tilbakemeldingen og
viser DENNE meldingens `mottakstatus` og `avvik` fra den. Innsending er
på når API-et har Maskinporten-klienten (`MASKINPORTEN_*`, scopene settes
av skinnen selv); uten den sier endepunktet det, og portalen tilbyr bare
filen.

**Prøvd ende til ende mot Skatteetatens testmiljø 2026-11-11** fra
portalen: «Send til Skatteetaten» → `levert` → «Hent tilbakemelding» →
`mottatt, 0 avvik`, for det syntetiske Tenor-selskapet 311032240.

### Tilbakemelding som hendelse (migration 0075)

Altinns bruksvilkår (§1.2.4) foretrekker hendelser framfor polling, og
Dialogporten publiserer en hendelse hver gang en forsendelse opprettes i
en dialog — også Skatteetatens tilbakemelding. Når API-et har
`ALTINN_EVENTS_ENDPOINT` (vår offentlige webhook, f.eks.
`https://api.test.regnmed.no/webhooks/altinn`), abonnerer regnmed
selskapet på `dialogporten.dialog.transmission.created.v1` for ressursen
`ske-innrapportering-amelding` første gang en a-melding leveres, én gang
per selskap (`altinn_event_subscription`, innsettings-bar, unik per
selskap/filter/endepunkt). Feiler abonnementet, logges det og
innsendingen står likevel — knappen er reserveveien.

**Events-API-et tar ikke et Maskinporten-token.** Det svarer 401, og
godtar det samme tokenet etter veksling til et Altinn-token på
`/authentication/api/v1/exchange/maskinporten` (prøvd mot TT02
2026-11-11). Vekslet fra et systembrukertoken kjører abonnementet SOM
systembrukeren (`consumer: /systemuser/<id>`), og det er det som gir
tilgang til kundens hendelser. Scopet `altinn:events.subscribe` ligger i
`amelding::SCOPES`, så det er samme token-leverandør som innsendingen.

**Webhooken `POST /webhooks/altinn` er åpen, og stoler derfor ikke på
noe den får.** Altinn verken autentiserer eller signerer leveransene. En
hendelse er bare et hint om hvilken dialog man skal se på: regnmed slår
opp leverte innsendinger som venter på tilbakemelding og har den
dialogen i kvitteringen, og henter tilbakemeldingen gjennom sin EGEN
systembruker-autentiserte klient. Valideringshendelsen
(`platform.events.validatesubscription`) besvares med 200, og det gjør
alle andre også — en ukjent dialog er ikke en feil Altinn skal prøve på
nytt. En falsk hendelse kan altså i verste fall få regnmed til å hente en
tilbakemelding som venter, aldri til å lagre noe den ikke selv har hentet.

Prøvd: API-testen spiller Altinn mot en etterligning (validering,
hendelser for fremmede dialoger og ressurser ignoreres, tilbakemeldingen
lagres uten knappetrykk, samme hendelse to ganger henter ikke på nytt,
en erstatning abonnerer ikke på nytt), og live mot TT02 er abonnementet
opprettet, lest tilbake og slettet gjennom vår egen klient.

**Prøvd ende til ende i testklyngen 2026-11-11**, uten knappetrykk: fra
test.regnmed.no ble oktober-meldingen til Tenor-selskapet 311032240
sendt; leveransen abonnerte selskapet selv (abonnement 126913 mot
`https://api.test.regnmed.no/webhooks/altinn`), og 15 sekunder senere
lå tilbakemeldingen lagret med `created_by = altinn-events` —
`mottatt`, 0 avvik — før noen trykket «Hent tilbakemelding». Altinn
validerte et abonnement mot den samme webhooken på under 5 sekunder.

## Hva som IKKE er bygget

Rekkefølgen er omtrent den de bør tas i.

1. **A-meldingen.** Månedlig, frist den 5. Renderen finnes og er
   validert mot XSD-en (se «A-meldingen» under), og arbeidsforholdets
   felt finnes nå (0073), og den lages fra en lagret kjøring med trekk
   og avgift i hele kroner, men den nekter arbeidsforhold uten
   lønn i måneden, og innsending venter på
   Altinn-systembrukeren (#180, docs/gov.md). Inntil videre er dette
   internt regnskap, ikke rapportering — **selskapet må fortsatt levere
   a-melding på annet vis.** Dette er den viktigste mangelen.
2. **Sykepengerefusjon, naturalytelser, pensjonstrekk, tariff-logikk,
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

Lønn-seksjonen har tre kort: **Ansatte** (register + nyregistrering),
**Skattekort** (hent fra Skatteetaten for et inntektsår; tabell over
hver ansatt med Skatteetatens svar, hva som ble satt og merknaden for det
som ikke ble satt) og **Lønnskjøring** (historikk + kjøreskjema med én rad per aktiv ansatt,
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

- Skattekortene: `regnmed-gov::tax_card` leser det EKTE svaret fra
  testmiljøet og fester beslutningen for hver korttype (prosent, tabell,
  frikort med og uten grense, kildeskatt, uten kort), og at prosenten
  aldri går gjennom et flyttall. Mot ekte Postgres
  (`tests/grupper/tax_card.rs`, en lokal Skatteetaten som svarer 204
  først): at bestillingen nektes uten vilkårsaksept og uten å sende noe,
  at den gjelder årets ansatte og ikke en som sluttet året før, at bare
  prosent- og tabellkortene settes på den ansatte mens de andre beholder
  trekket med merknad, at et nytt svarkall ikke henter noe på nytt, og
  at kortet ikke kan endres. Testen er sett feile med 10,5-månedsvakten
  fjernet.
- `regnmed-core::withholding_table` — hver rad i utvalget fra
  Skatteetatens publiserte 2026-fil (og hele fila, 1 063 799 rader, når
  `TREKKTABELL_FILE` er satt — kjørt 2026-11-12, alle like), rutinens egne
  kontroller (trygdeavgiftsgrensene 125 188 / 143 175, aldri mer enn
  grunnlaget), og at ukjente tabeller og år nektes.
- `regnmed-core::payroll` — prosenttrekk, tabelltrekk (og halvt i
  desember), trekkfrie feriepenger, halv skatt i desember, kort over 12
  måneder (trekk i feriepenger, fullt i desember), frikort med grense
  (under, over og på vei over), trekk og arbeidsgiveravgift avrundet
  ned til hele kroner (Skatteetatens 1 727,25 → 1 727, og ned også ved
  ,99), og at ukjente tabeller, år og sone Ia nektes med begrunnelse.
- Mot ekte Postgres: frikort over to kjøringer (januar under grensen,
  februar 50 % av det som overstiger) med fjorårets lønn utenfor
  tellingen, at en kjøring utbetalt i et år uten hentet frikort nekter,
  et 12-månederskort med feriepenger, at lønnsslippen bærer metoden
  fra kjøringen, og at to lønninger med øre gir avgift regnet én gang på
  summen (10 481 kr, ikke 10 480 per ansatt) som a-meldingen godtar.
- Aga-avsetningen: at målet er satsen av gjelden, at livsløpet
  (avsetning → utbetaling) summerer til nøyaktig null, at en satsendring
  korrigeres i sin helhet ved neste kjøring, og at negativ gjeld gir
  null avsetning framfor negativ avgift.
- At startkontoplanen bærer hver `payroll::ACCOUNT_*` (enhetstest i
  `regnmed-db::marketplace`), og mot ekte Postgres at en kjøring med to
  deaktiverte kontoer nekter med begge numrene og ingen kjøring lagret.
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
  øre der skjemaet vil ha kroner nektes framfor å rundes (en kjøring med
  avgift i øre fra før avrundingen nektes som F185); og at koder
  utenfor kodelisten, ugyldig fødselsnummer og orgnr nektes. Etter
  review: at kalendermåneden følger utbetalingsdatoen (også over
  årsskiftet, der trekkformatet skifter), at avgift som ikke er grunnlag
  × sats nektes, at negative beløp nektes, at meldingsId, kildesystem og
  år utenfor XSD-ens grenser nektes, at leveringstidspunktet har `Z`, og
  en melding med to mottakere der den ene har frikort.
- `regnmed-core::payslip` — velformet og deterministisk PDF, at
  slippen forklarer trekkgrunnlaget, feriepengenes trekkfrihet og
  desembers halve trekk, at den lagrede metoden står og at en linje
  uten metode ikke gjetter en, og at
  fødselsnummeret ikke er i dokumentet.
- Lønnsslippen bygget fra en ekte kjøring, med hittil-i-år summert over
  flere måneder — og hentet som `application/pdf` fra en kjørende
  server (2,8 kB, uten fødselsnummer).
