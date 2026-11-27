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

- **Feriepenger er trekkfrie** når de utbetales i ferieåret, innenfor
  grensene i neste avsnitt, og aldri for tillegget til arbeidstakere over
  60 år (se «Forskuddstrekk på tillegget»).
- **Halv skatt i desember** (skattebetalingsloven,
  skattebetalingsforskriften § 5-7-2). Siden 2016 kan
  arbeidsgiver i stedet ta det i annen halvdel av november; regnmed
  gjør desember, det vanlige valget, og sier det her. Fritaket følger
  **lønnsperioden**, ikke utbetalingsdatoen: for månedslønn gjelder det
  «for» første halvdel av desember (§ 5-7-2 første ledd), mens bare
  14-dagerslønn er knyttet til selve utbetalingen (tredje ledd).
  Desemberlønn utbetalt i januar får derfor halvt trekk, og novemberlønn
  utbetalt i desember fullt. Forskriften nevner ikke lønn utbetalt i en
  annen måned enn den gjelder, så dette er vår lesing av ordlyden og bør
  bekreftes med Skatteetaten.

### Feriepenger det likevel skal trekkes i

Fritaket i skattebetalingsforskriften § 5-7-1 (1) gjelder utbetaling i
ferieåret, altså kalenderåret etter opptjeningsåret. På kort over 10,5
måneder faller to tilfeller utenfor, uansett alder:

1. **Fratreden i opptjeningsåret** (§ 5-6-1 (1) a). Feriepenger som
   utbetales i opptjeningsåret fordi den ansatte slutter, er trekkpliktige.
   Det gjelder også avtalefestede feriepenger.
2. **Mer enn 21 % over ferielovens beløp** (§ 5-7-1 (1) b). Avtalefestede
   feriepenger er fritatt opp til 10,2 % × 1,21 = 12,342 % av grunnlaget, og
   det overskytende er trekkpliktig. Grensen regnes per sats linjene faktisk
   ble avsatt med, ikke etter den ansattes sats i dag. Skattebetalingshåndboken § 5-7 runder
   dette til 12,34 %; regnmed bruker forskriftens eksakte grense. Med 15 %
   av 1 000 000 kr er 123 420 kr fritatt og 26 580 kr trekkpliktig. Satsene
   12 % og 14,3 % ligger under grensen, fordi den ordinære delen av 14,3 %
   er 12 %. Tillegget på 2,3 prosentpoeng for arbeidstakere over 60 år har
   egen hjemmel (§ 5-6-1 (1) b) og telles ikke mot grensen.

**Hvilket år en utbetaling hører til** regnes av de innsettings-bare
lønnslinjene, etter utbetalingsdato (`payroll::split_holiday_pay`). Eldste
gjeld dekkes først. Ved fratreden forfaller alt samme år (ferieloven § 11
nr. 3), så rekkefølgen flytter bare beløp mellom måneder, ikke mellom år.
Grensen på 21 % regnes kumulativt, så samme utbetaling delt i to gir samme
overskytende beløp. Hvor mye som er trukket som årets egne feriepenger,
lagres på linjen (`holiday_pay_earning_year_ore`, migration 0106), slik at
en senere utbetaling samme år ikke kan telle de samme feriepengene to ganger.

**Hvem som slutter.** En utbetaling regnes som utbetalt på grunn av
fratreden når sluttdatoen ligger før neste månedlige lønningsdag, fordi
feriepengene forfaller siste vanlige lønningsdag før fratreden (ferieloven
§ 11 nr. 3). Oppsigelse til oktober gjør ikke juniutbetalingen til en
sluttutbetaling, mens sluttdato 5. januar gjør 20. desember til det. Den som
betaler siste dag i måneden, har neste lønningsdag siste dag i neste måned,
så 28. februar følges av 31. mars.

**En åpningsbalanse er usynlig i lønnshistorikken.** Derfor regnes et beløp
over det tidligere år skylder, som årets feriepenger *bare* for den som
slutter, og aldri mer enn det historikken viser er opptjent i år. Ellers
ville hver juniutbetaling i et migrert selskap blitt trukket. Det som
fortsatt ikke kan forklares, behandles som trekkfritt, og kjøringen sier
fra med beløpet når den ansatte slutter.

**Kjent begrensning.** Historikken skiller ikke et forskudd på årets
feriepenger til en som blir værende fra utbetaling av en åpningsbalanse.
Et slikt forskudd trekkes ikke, selv om § 5-7-1 ikke fritar det, og årets
avsetning ser fortsatt skyldig ut neste år, så en senere fratreden kan bli
trukket for lite. Forskudd må derfor trekkes for hånd.

**Trekkmåten.** Prosentkort bruker kortets prosent. På tabellkort kan
disse beløpene ikke gå gjennom tabellen, siden de er opptjent over et
vesentlig lengre tidsrom enn månedens lønn (§ 5-10-1 (4)). De trekkes med
trekkprosenten på tabellkortet (§ 5-10-3), av hele kroner (§ 5-10-4).
regnmed lagrer den prosenten når skattekortet hentes, og migration 0107
henter den fra kort som allerede er lagret. Mangler den, nekter kjøringen og
ber om at kortet hentes på nytt; den kan ennå ikke føres inn for hånd.
Beløpene halveres ikke i
desember: bare ordinær lønn innenfor gjennomsnittet får halvt trekk (se «Hva
det halve trekket i desember dekker»), som er siden som ikke gir den ansatte
restskatt.

**Unntak den som kjører lønn må merke.** Feriepenger ved dødsfall
(§ 5-7-1 (1) d) og feriepenger som etter avtale utbetales regelmessig med
lønnen (§ 5-7-1 (1) c) er trekkfrie også i opptjeningsåret. regnmed vet ikke
hvorfor et arbeidsforhold endte, så skjemaet har en avhuking for ansatte med
sluttdato (`feriepenger_trekkfri` i API-et). Kjøringen minner om den når den
trekker i feriepenger ved fratreden. Merkingen fjerner trekk, så den lagres på
lønnslinjen (`holiday_pay_exempt`, migration 0107) og står i trekkmåten på
lønnsslippen. En merket utbetaling regnes som oppgjør av årets egne
feriepenger, så den ikke ser ubetalt ut neste år. På kort over 12 måneder og
frikort har merkingen ingen virkning, og kjøringen sier fra.

**Ikke bygget:** valget i § 5-7-1 (2) om å gi trekkfritaket som én vanlig
månedslønn. regnmed følger første ledd. A-meldingen er uendret: feriepenger
rapporteres som én trekkpliktig `feriepenger`-linje uansett.

### Hva det halve trekket i desember dekker

Skattebetalingsforskriften § 5-7-2 gir ikke «halv skatt på alt som
utbetales i desember». Fritaket er avgrenset, og der forskriften og
Skattebetalingshåndboken tier, tar regnmed den siden som ikke kan gi den
ansatte restskatt. For mye trekk kommer tilbake i skatteoppgjøret; for lite
blir en regning. Kjøringen sier fra med en advarsel per ansatt hver gang
desember ikke får fullt halvt trekk.

| Tilfelle | Hva kjøringen gjør | Grunnlag |
| --- | --- | --- |
| Månedslønn innenfor gjennomsnittet | Halvt trekk | § 5-7-2 første ledd |
| Bonus eller tillegg som løfter måneden over gjennomsnittet | Halvt trekk opp til gjennomsnittet, fullt på resten | § 5-7-2 fjerde ledd |
| Timelønn i en månedskjøring | Fullt trekk | § 5-7-2 annet ledd fritar to bestemte uker. **Vårt valg** |
| Beløp der kjøringen ikke kan se lønnsformen | Fullt trekk | **Vårt valg** |
| Ingen tidligere lønnsmåned uten feriepenger i regnmed | Fullt trekk | Fjerde ledd krever et gjennomsnitt. **Vårt valg** |
| Andre halve trekk i samme utbetalingsår | Fullt trekk | Kildene er tause. **Vårt valg** |

**Gjennomsnittet.** Fjerde ledd sier at fritaket gjelder «så langt den
samlede ytelsen ikke overstiger arbeidstakerens gjennomsnittsfortjeneste hos
arbeidsgiveren i denne del av inntektsåret», men ikke hvordan det regnes.
regnmed tar ordinær lønn og trekkpliktig godtgjørelse for årets lønnsperioder
før desember og deler på antall slike måneder, rundet ned. To valg er våre:

- **Lønnsperioder, ikke utbetalingsdatoer**, så desemberlønn utbetalt i januar
  måles mot sitt eget år.
- **Måneder med feriepenger telles ikke.** Ferietrekket ville ellers gjort at
  en ansatt med helt jevn lønn så ut til å ligge over sitt eget gjennomsnitt
  hver desember. Tas ferietrekket i en annen måned enn feriepengene, kan
  kjøringen ikke se det. Da blir snittet lavere og fritaket mindre, som er
  den sikre siden, og advarselen viser beløpet.

Eksempel: 50 000 kr i lønn og 100 000 kr i bonus på et kort med 35 %, med
50 000 kr i gjennomsnitt. 50 000 kr trekkes med 17,5 % og 100 000 kr med 35 %,
til sammen 43 750 kr. Tidligere ble hele beløpet halvert til 26 250 kr.
Skattebetalingshåndboken beskriver ikke regnestykket. Simployer regner
gjennomsnittet på tilleggene alene, mens forskriften sier «samlede ytelsen».
Med jevn grunnlønn gir de to samme svar, og ved en lønnsøkning gir vår lesing
mindre fritak, altså den sikre siden.

På **tabellkort** er fritaket halvparten av tabelltrekket på beløpet innenfor
gjennomsnittet: tabelltrekket på hele måneden minus halve tabelltrekket på
gjennomsnittet. Forskriften regner ikke dette ut. Tabellen er progressiv, så
regnmed lar hele beløpet slå ut i trekket og trekker bare fra den halvparten
en vanlig måned ville fått.

**Timelønn.** Annet ledd fritar «to uker i siste halvdel av november eller
siste uke i november og første uke i desember». Ingen kilde sier hva en
månedskjøring av timelønn skal gjøre. Halv måned er ikke to uker, og den ene
av de to ukene ligger i novemberkjøringen, som allerede er trukket. regnmed
trekker derfor fullt og sier fra. Det betyr at timelønnede ikke får fritaket
utbetalt før jul, men får det igjen i skatteoppgjøret.

**To halve trekk i samme år.** Flyttes lønnsdagen over nyttår, kan to
desemberlønner utbetales i samme inntektsår (desember 2026 i januar 2027 og
desember 2027 i desember 2027). Forskriften tar ikke stilling til det.
Skattekortet er regnet for ett halvt trekk per år, så den andre trekkes fullt.

Alt dette er vår lesing av forskriften og bør bekreftes med Skatteetaten.

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
| `feriepenger_over_60` | 12,5 % | ferieloven §10 nr. 3 (+2,3 prosentpoeng, tillegget bare opp til 6 G) |
| `folketrygdens_grunnbelop` | 124 028 kr fra 1.5.2024, 130 160 kr fra 1.5.2025, 136 549 kr fra 1.5.2026 | FOR-2024-05-24-815, FOR-2025-05-23-837 § 1, FOR-2026-05-22-869 § 1 |

Meldingen sier selv at verken soneinndeling eller satser er endret fra
2025 til 2026, så periodene starter 2025-01-01 — den tidligste
**verifiserte** datoen, ikke gjettet historikk.

Feriepengesatsen ligger per ansatt, fordi den er et faktum om
arbeidsforholdet (alder, tariff), ikke en systeminnstilling.

## Feriepenger for arbeidstakere over 60 år

**Bare tillegget stopper ved 6 G.** Ferieloven § 10 nr. 2 gir 10,2 % av
hele grunnlaget. § 10 nr. 3 første ledd forhøyer satsen med 2,3
prosentpoeng, og andre ledd sier at feriepenger «etter første ledd» ikke
ytes for den delen av grunnlaget som overstiger 6 G. Forarbeidene beskriver
det som en begrensning «for ekstraferie» (Ot.prp. nr. 65 (2007–2008) pkt.
6.2.1), og Skatteetatens a-meldingsveiledning oppgir «supplement 2.3 percent
up to 6 National Insurance basic amounts (G)». Med 1 000 000 kr i grunnlag
for opptjeningsåret 2026 blir feriepengene 102 000 kr + 2,3 % av 819 294 kr
= 120 843,76 kr.

**Hvilket år tillegget starter.** Retten følger av at arbeidstakeren
fyller 60 år i ferieåret (§ 5 nr. 2), og feriepengene for ferieåret beregnes
av lønn utbetalt i opptjeningsåret, altså året før (§ 4 og § 10 nr. 1).
Lovavdelingen legger det samme til grunn i sin uttalelse 23. mai 2009
(sak 200902697): den forhøyede satsen beregnes av grunnlaget tjent opp året
før. Den som fyller 60 i 2027 skal derfor ha tillegget på lønn utbetalt i
2026, året vedkommende fyller 59. Satsen velges fortsatt per ansatt, og
regnmed hever den ikke av seg selv. Står den ansatte på 10,2 % eller 12 % fra
det året, viser ansattlisten satsen som kreves (12,5 % eller 14,3 %), og hver
kjøring advarer til satsen er endret.

**Når satsen endres.** Satsen endres under Ansatte
(`PUT …/employees/{eid}/holiday-rate`, `LONN_SKRIV`) og gjelder fra neste
kjøring. Hver lønnslinje lagrer satsen den ble avsatt med
(`payroll_line.holiday_pay_bp`, migration 0105), så en lønnsslipp viser alltid
satsen som faktisk ble brukt. Linjer fra før migrasjonen endres ikke. De leser
satsen den ansatte hadde da migrasjonen kjørte, som var den eneste satsen de
kunne ha fått.

Tillegget gjelder hele opptjeningsåret. Heves satsen fra 10,2 % til 12,5 %
eller fra 12 % til 14,3 % midt i året, tar **første kjøring etter endringen
igjen** 2,3 % av lønnen som alt er utbetalt i året, og kjøringen sier fra med
beløpet. Den lønnen teller også mot 6 G. Etter året er tillegget dermed
2,3 % av det laveste av årets grunnlag og 6 G, uansett når satsen ble hevet.
Eksempel: 300 000 kr utbetalt på 10,2 %, så 600 000 kr på 12,5 %, gir
91 800 kr + 2,3 % av 819 294 kr = 110 643,76 kr.

Lønn på en annen avtalt sats, for eksempel 13 %, teller verken mot 6 G eller
gir etterbetaling, siden regnmed ikke kan se om den satsen allerede rommer
tillegget. To kjøringer samtidig kan ikke begge ta igjen det samme beløpet: den
som kommer sist nektes og kjøres på nytt.

Andre endringer virker bare framover: en overgang fra 10,2 % til 12 % etter
ny tariffavtale tar ikke igjen noe, og en senket sats endrer ikke det som alt
er avsatt. Et **tidligere** opptjeningsår med for lav sats rettes ikke
automatisk, fordi det året kan være utbetalt og innrapportert. Advarselen sier
fra, og differansen må føres for hånd.

**Hvilken G.** Grunnbeløpet per 31. desember i opptjeningsåret, eller på
fratredelsestidspunktet for den som slutter i opptjeningsåret (§ 10 nr. 3
andre ledd). Før ny G er fastsatt i mai brukes den siste registrerte. Hver
kjøring regner årets kutt på nytt fra alt som er utbetalt i året, og trekker
fra det tidligere linjer allerede har kuttet (`holiday_pay_cap_cut_ore`),
så en ny G retter seg selv ved neste kjøring. Bokførte linjer endres aldri.
Mangler årets G etter fristen i satsregisteret, advarer kjøringen når taket
slår inn.
Lønn teller i det året den er **utbetalt**, så desemberlønn utbetalt i januar
hører til det nye året.

**Tariff.** 12 % har ikke noe 60-årstillegg og derfor ikke noe tak. 14,3 %
er 12 % + 2,3 prosentpoeng: 12 % løper på all lønn og tillegget stopper ved
6 G, slik Skatteetatens tabell viser. § 10 nr. 6 lar en tariffavtale fravike
beregningsreglene, og Skatteetaten nevner at tariff eller annen avtale kan gi
høyere grunnlag. regnmed vet ikke hva den enkelte avtalen sier, så kjøringen
**advarer med beløpet** hver gang taket slår inn. Gir avtalen høyere grunnlag,
må differansen avsettes for hånd. Andre satser enn 12,5 % og 14,3 % behandles
som avtalte satser uten tak. Det samme gjelder disse to satsene før
opptjeningsåret den ansatte fyller 59.

Kilder: [ferieloven](https://lovdata.no/dokument/NL/lov/1988-04-29-21),
[Ot.prp. nr. 65 (2007–2008)](https://www.regjeringen.no/no/dokumenter/otprp-nr-65-2007-2008-/id516695/?ch=6),
[Lovavdelingens uttalelse](https://www.regjeringen.no/no/dokumenter/-5---tolkning-av-ferieloven---innforing-/id571024/),
[Skatteetaten om feriepenger i a-meldingen](https://www.skatteetaten.no/en/business-and-organisation/employer/the-a-melding/the-a-melding-guide/salary-and-benefits/overview-of-salary-and-other-benefits/holiday-pay/).

### Forskuddstrekk på tillegget

Feriepenger utbetalt i ferieåret er trekkfrie, men **tillegget på 2,3
prosentpoeng er trekkpliktig** (skattebetalingsforskriften § 5-6-1 første ledd
bokstav b). Fritaket i § 5-7-1 første ledd bokstav a gjelder feriepenger etter
ferieloven § 10 nr. 2 og nevner ikke nr. 3, og Skattebetalingshåndboken sier at
det «alltid» skal trekkes i tillegget.

| Sats | Trekkfritt | Trekkpliktig |
| --- | --- | --- |
| 12,5 % | 10,2 % av hele grunnlaget | 2,3 % av grunnlaget opp til 6 G |
| 14,3 % (tariff) | 12 % av hele grunnlaget | 2,3 % av grunnlaget opp til 6 G |

Med 1 000 000 kr i grunnlag og 6 G på 819 294 kr er 102 000 kr trekkfritt og
18 843,76 kr trekkpliktig på 12,5 %. På 14,3 % er 120 000 kr trekkfritt og det
samme beløpet trekkpliktig. Tariffens 12 % ligger under grensen på 12,34 % av
grunnlaget (§ 5-7-1 første ledd bokstav b). Kort over 12 måneder trekker i alle
feriepenger som før.

**Hele fjorårets opptjening regnes ut.** Er utbetalingen nøyaktig det som ble
avsatt i fjor og fortsatt er skyldig, er tillegget det som faktisk ble avsatt
over den ordinære satsen. Da er 6 G-taket allerede med.

**Alt annet må oppgis.** Ingen kilde sier om en delutbetaling går først til den
trekkfrie delen, til tillegget eller forholdsmessig, så regnmed fordeler ikke.
Kjøringen nekter til tillegget er oppgitt (`holiday_pay_supplement_ore`, i
portalen «Herav tillegg over 60») ved delutbetaling, rest etter en tidligere
utbetaling, feriepengegjeld uten lønnshistorikk i regnmed og avtalte satser der
vi ikke ser delene (fra 14,642 % dekker det som trekkes over grensen et helt
tillegg uansett, så der trengs det ikke). Feilmeldingen oppgir hva som er skyldig fra tidligere år.
Andelen regnes av satsen hver lønnslinje ble avsatt med
(`payroll_line.holiday_pay_bp`), så en sats som er endret etter opptjeningen
flytter den ikke.

**Merket trekkfri.** Tillegget trekkes også når utbetalingen er merket trekkfri
for dødsfall eller løpende utbetaling (§ 5-7-1 første ledd bokstav c og d).
§ 5-6-1 første ledd bokstav b har ikke noe unntak, og ingen av bestemmelsene
sier hvilken som går foran, så regnmed velger siden som ikke gir restskatt.
Den som kjører lønn, kan oppgi 0 i «Herav tillegg over 60» når Skatteetaten har
sagt at tillegget er fritatt. Holder utbetalingen også årets opptjening, må
tillegget oppgis, siden beløpet da dekker to år.

**Over 12,34 %.** Tillegget er den delen som ligger over den ordinære satsen,
så det trekkes bare én gang: det som går til tillegget, regnes ikke også som
overskudd over grensen. **Kjent begrensning:** kjøringen ser bare tillegget i
den utbetalingen den regner på. En avtalt sats over grensen som utbetales i
flere deler med oppgitt tillegg, kan derfor bli trukket for mye. Det rettes i
skatteoppgjøret.

**Tabellkort er en tolkning.** Tillegget legges til månedens lønn, og trekket
leses av tabellen én gang. § 5-10-2 annet ledd legger trekkpliktig
feriegodtgjørelse til arbeidstakere over 60 år under tabellreglene i første
ledd, § 5-10-1 første ledd hindrer to tabelloppslag for samme periode, og Sticos
og Simployer beskriver tillegget som en del av trekkgrunnlaget ved utbetaling.
Skatteetatens generelle veiledning sier derimot at «ytelser som gjelder annen
periode enn den lønnen betales ut for» skal bruke kortets prosent. Den nevner
ikke dette tillegget, og vi legger den særskilte forskriftsteksten til grunn.
Ingen kilde sier det uttrykkelig, så punktet bør bekreftes av Skatteetaten.

**Desember.** Halvt trekk gjelder den ordinære lønnen innenfor
gjennomsnittsfortjenesten, ikke tillegget. Kildene omtaler ikke tillegget her,
så regnmed velger siden som ikke gir den ansatte restskatt.

**A-meldingen** har én `feriepenger`-linje med hele beløpet, alltid trekkpliktig.
Skatteetatens veiledning har ingen egen linje for tillegget. Det som trekkes,
inngår i forskuddstrekket.

Kilder: [skattebetalingsforskriften § 5-6-1](https://lovdata.no/forskrift/2007-12-21-1766/§5-6-1),
[§ 5-7-1](https://lovdata.no/forskrift/2007-12-21-1766/§5-7-1),
[§ 5-10-2](https://lovdata.no/forskrift/2007-12-21-1766/§5-10-2),
[Skattebetalingshåndboken](https://oppslag.rettskilder.skatteetaten.no/rettskilder2/type/handboker/skattebetalingshandboken/gjeldende/SKTB_ID-5-0.001/SKTB_ID-5-9.001/SKTB_ID-5-9.008),
[Slik trekker du riktig skatt](https://www.skatteetaten.no/bedrift-og-organisasjon/arbeidsgiver/skattekort-og-skattetrekk/slik-trekker-du-riktig-skatt/),
[Sticos](https://www.sticos.no/fagstoff/slik-er-reglene-feriepenger-til-ansatte-over-60-aar),
[Simployer](https://www.simployer.com/no/artikler/skatt-feriepenger-60).

**Den ekstra arbeidsgiveravgiften på 5 % over 750 000 kroner ble fjernet
fra 2025.** Den finnes ikke i registeret fordi den ikke finnes.

## Trekk satt for hånd

Skattekortet er hovedveien. Skjemaet for ny ansatt har valget **Hent fra
skattekort** der henting er satt opp, og alternativene **Prosent**,
**Tabell** og **Ingen trekk**.

- **Hent fra skattekort** registrerer den ansatte med 50 % trekk, som er
  det arbeidsgiver skal trekke uten skattekort, og bestiller så kortet
  for akkurat denne personen (`POST …/tax-cards?year=&employee=`). Feiler
  bestillingen, står den ansatte med 50 %, og portalen sier fra.
- **Tabell** godtar bare numre trekkrutinen kjenner
  (`withholding_table::Table::lookup`).
- **Frikort og prosent over 12 måneder** kan ikke settes for hånd, fordi
  de bærer en beløpsgrense eller vilkår som bare står på skattekortet.

Trekket på en eksisterende ansatt endres med `PUT
…/employees/{eid}/withholding` (`LONN_SKRIV`), og **begrunnelse er
påkrevd**. Endringen og raden i den innsettings-bare
`employee_withholding_change` (migration 0108: fra, til, hvorfor, hvem,
når) skrives i samme transaksjon. Sporet leses med `GET` på samme adresse
(`LONN_LES`). Bokførte kjøringer beholder trekket de brukte, og et nytt
skattekort erstatter det som er satt for hånd.

Å sette trekket for hånd på en ansatt med frikort eller prosent over 12
måneder fjerner kortets vilkår, og skjemaet advarer om det før lagring.
Trekkprosenten på et tabellkort, som brukes på feriepenger utenfor
fritaket, består når samme tabell lagres på nytt. Et annet tabellnummer
fjerner den, og kjøringen nekter da slike feriepenger til kortet er hentet.
Et kort som ikke ble satt, gir en advarsel i lønnskjøringen til noen har
satt trekket for hånd med begrunnelse. Skattekort-kortet i portalen viser
nyeste kort per ansatt for året og hver bestilling som fortsatt venter
på svar, så en enkeltbestilling ikke skjuler en bestilling for alle.

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
10,5-månederstabellene er regnet for det, og med samme avgrensning (se
«Hva det halve trekket i desember dekker»). Feriepenger er trekkfrie på
samme vilkår som for prosentkort; det som faller utenfor, trekkes med
kortets trekkprosent (se «Feriepenger det likevel skal trekkes i»).
Tillegget for arbeidstakere over 60 år går derimot inn i månedens grunnlag
og halveres ikke i desember.

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
| Trekktabell | **settes** når tabellnummeret og kortet er enige om 10,5 eller 12 måneder, sammen med kortets trekkprosent; ellers beholdes |
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

**Prøvd fra deployet test 2026-11-23:** portalen bestilte skattekortet til en
av Skatteetatens publiserte testpersoner for et syntetisk selskap (`BR29541`,
skattekort-id 340079). Kortet på 38 % over 10,5 måneder ble lagret og satt på
den ansatte. Forhåndsvisningen av en lønn på 40 000 kr trakk 15 200 kr i
november og 7 600 kr (halv skatt) i desember.

Samme dag gikk kortet hele veien i et annet syntetisk selskap (`BR29563`). En
bokført novemberlønn på 40 000 kr i sone IV førte 15 200 kr på konto 2600, og
hash-kjeden ble verifisert. A-meldingen ble sendt fra test, og Skatteetaten svarte
`mottatt` uten avvik, med 15 200 kr i forskuddstrekk og 2 040 kr i
arbeidsgiveravgift. Halv skatt i desember er bare prøvd i forhåndsvisningen, og
andre korttyper er ikke satt på en ansatt fra deployet test.

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
| PUT | `/companies/{id}/employees/{eid}/holiday-rate` (feriepengesats fra neste kjøring) |
| POST/DELETE | `/companies/{id}/employees/{eid}/link` (koble/koble fra portalbruker) |
| GET | `/companies/{id}/employees/{eid}/link/history` |
| GET/POST | `/companies/{id}/payroll` (`?preview=true`: beregn uten å bokføre) |
| GET | `/companies/{id}/payroll/{run}/slip/{employee}` (PDF) |
| GET | `/companies/{id}/payroll/{run}/amelding` (`?format=xml`) |
| GET/POST | `/companies/{id}/payroll/{run}/amelding/submissions` |
| GET | `/companies/{id}/payroll/months/{ÅÅÅÅ-MM}/amelding` (måned uten kjøring) |
| GET/POST | `/companies/{id}/payroll/months/{ÅÅÅÅ-MM}/amelding/submissions` |
| GET | `/companies/{id}/payroll/hours/{employee}?ar=&maned=` |
| GET/POST | `/companies/{id}/payroll/garnishments` (trekkpålegg, docs/trekkpalegg.md §2 og §6) |
| POST | `/companies/{id}/payroll/garnishments/fetch` (hent fra Skatteetaten) |
| POST | `/companies/{id}/payroll/garnishments/letters?filename=` (brevet, rå kropp) |
| GET | `/companies/{id}/payroll/garnishments/letters/{brev}` |
| PUT | `/companies/{id}/payroll/garnishments/{pålegg}` (ny versjon) |
| POST | `/companies/{id}/payroll/garnishments/{pålegg}/stop` |
| POST | `/companies/{id}/payroll/garnishments/{pålegg}/versions/{n}/attest` |
| PUT | `/companies/{id}/payroll/garnishment-settings` |
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
Innsendingen er bygget (se «Innsendingssporet» under) og prøvd mot
Skatteetatens testmiljø; i produksjon venter den på
Altinn-systembrukeren (#180, docs/gov.md). Fra en lagret kjøring lages
meldingen av `regnmed-db::payroll::amelding_for_run`, se «Fra lagret
kjøring» under; for en måned UTEN kjøring av `amelding_for_month`, se
«Måned uten lønnskjøring».

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
| `inntekt` kjøregodtgjørelse (`utgiftsgodtgjoerelse`, `antall` = km) | `payroll_expense` — trekkfri del (gammel kode 711), del over trekkfri sats (153, utløser aga og trekk), passasjertillegg og andre kjøretøy (714). Se docs/utlegg.md «Utbetaling» |
| `inntekt` diett (`utgiftsgodtgjoerelse`, `antall` = døgn på de trekkfrie) | `payroll_travel` — trekkfri kost per overnattingstype og dagreise, trekkfritt nattillegg, og `reiseKost`/`reiseAnnet` for det over trekkfri sats (utløser aga og trekk, uten `antall`). Se docs/utlegg.md «Reise og diett» |
| `forskuddstrekk` (`ordinaert`, negativt, F278-2) | `payroll_line.forskuddstrekk_ore` |
| `sumForskuddstrekkPerLoennsutbetalingsdato` (fra 2026-01, F523) | `payroll_run.utbetalt_dato` + sum trekk |
| `sumArbeidsgiveravgift` | `payroll_run.aga_ore` (ikke avsetningen på feriepenger — den er periodisering, ikke avgift å betale). Må være NØYAKTIG grunnlaget × satsen avrundet ned til hele kroner (F185, skattebetalingsforskriften § 10-4-4); ellers nektes meldingen med begge tallene |
| `loennOgGodtgjoerelse`: beregningskode, sone, grunnlag, sats | `generelleNaeringer`; `payroll_run.sone` (I→`1`, Ia→`1a`, …); summen av inntektene som utløser aga (F184) — ikke den trekkfrie kjøregodtgjørelsen eller dietten; satsen fra satsregisteret på utbetalingsdatoen |

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
| `yrke` | `occupation_code` | F115B: 7 siffer (STYRK-08) — bare formatet sjekkes; katalogen brukes til søk, ikke til å nekte en kode |
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
har ingen av dem, og serveren fyller aldri inn noe for dem: det som
mangler, mangler synlig til noen lagrer skjemaet.

**Forslag i skjemaet.** Portalen fyller inn forslag der kilden er god
nok, synlig og redigerbart. Ingenting lagres før brukeren trykker lagre,
og feltene for arbeidsforholdet står åpne når en ny ansatt registreres.

| Felt | Forslag |
| --- | --- |
| Feriepengesats | Satsen flest ansatte i selskapet har, ellers lovens 10,2 %. Ingen offentlig kilde oppgir satsen for den enkelte |
| Virksomhet | Selskapets eneste åpne underenhet i Enhetsregisteret (`GET …/business-units`). Har det flere, velges blant dem, med sist ansattes enhet som forslag |
| Yrkeskode | Ingen kode foreslås. Feltet søker på tittel eller kode i SSBs katalog (`GET …/occupations`) |
| Stillingsprosent, timer, arbeidstidsordning, ansettelsesform | Fra sist ansatte med komplett arbeidsforhold. Første ansatte får 100 %, ikke skift og fast, mens timer står tomt |
| Siste lønnsendring | Startdatoen for en NY ansatt. For en eksisterende ansatt gjettes den ikke, siden datoen kan ha endret seg siden start |

Feriepengeforslaget teller bare ansatte som fortsatt er ansatt og som
ikke fyller 59 år eller mer i år, så tillegget for eldre arbeidstakere
ikke smitter over på en nyansatt. Et forslag kan byttes ut når
Enhetsregisteret svarer, men en verdi brukeren selv har skrevet eller
tømt, røres aldri.

Et brudd hos Enhetsregisteret gir bare en tom forslagsliste, og feltet
kan fortsatt fylles ut for hånd.

`docs/lonn/yrkeskoder-ssb-145.csv` er SSBs Klass-klassifikasjon 145
(«Standard for yrkesklassifisering», STYRK-08) per 2026-11-23, 7 130
koder, redusert til `kode;navn` og lagret som UTF-8. Kilde:
`https://data.ssb.no/api/klass/v1/classifications/145/codesAt.csv?date=2026-11-23&csvSeparator=;`.
Listen hentes på nytt ved den årlige regelverksrevisjonen.

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

### Arbeidsforhold uten lønn i måneden (F108)

Et arbeidsforhold rapporteres hver måned til og med måneden sluttdatoen
er meldt i, også en måned uten utbetaling (F108: «Alle arbeidsforhold må
rapporteres hver måned til og med kalendermåneden inntektsmottaker
slutter»). Alle som er ansatt i meldingens kalendermåned uten å ha en
linje i kjøringen, tas derfor med som inntektsmottaker med
**arbeidsforholdet alene** — uten inntekt og uten trekk, noe XSD-en
tillater. De påvirker verken grunnlaget eller avgiften. Arbeidsforholdet
må være komplett som for alle andre, ellers er det en hindring.

Måneden er **utbetalingsdatoens**, ikke lønnsmånedens, samme regel som
kalendermåneden: marslønn utbetalt 2. april er aprils melding, så den
som begynte 1. april er med og den som sluttet 31. mars er ikke.

**Sluttdatoen** settes med `PUT /companies/{id}/employees/{eid}/end`
(`LONN_SKRIV`, `{"ansatt_til": "2026-04-30"}`; `null` fjerner en
feilført dato, og nøkkelen er påkrevd så en feilstavet nøkkel ikke kan
fjerne den) — i portalen «Sluttdato …» i ansattlisten. Uten den ville en
som har sluttet, blitt rapportert som ansatt hver måned. Sluttdatoen kan
ikke ligge før startdatoen. Lønn etter sluttdatoen — feriepenger året
etter — kjøres og rapporteres som før, med sluttdatoen på
arbeidsforholdet.

**En sluttdato som føres for sent** fanges også. Sluttet en ansatt i
januar, men februars melding rapporterte arbeidsforholdet uten
sluttdato fordi ingen hadde ført den, holder Skatteetaten det åpent —
og F108 krever det da igjen i mars. Meldingen tar derfor med enhver
ansatt som har sluttet før måneden, men som sto **uten sluttdato** i en
melding **levert** for forrige måned (med kvittering, og ikke erstattet
av en annen levert melding), nå med sluttdatoen. Kilden er den lagrede
meldingen selv (`amelding_submission.body`, nøyaktig det som ble sendt),
ikke en gjetning. En forrige måned som ble levert utenfor regnmed, ser
regnmed ikke.

### Måned uten lønnskjøring

En måned der INGEN får lønn, har ingen kjøring å lage meldingen av —
men arbeidsforholdene skal rapporteres likevel (F108; Skatteetaten:
a-melding skal sendes når arbeidsgiveren har aktive arbeidsforhold, selv
om det ikke er utbetalt lønn den måneden). `amelding_for_month` lager
den: hvert arbeidsforhold som er ansatt i måneden (og en sluttdato ført
for sent, som over), med **arbeidsforholdet alene**. Meldingen har ingen
lønnsutbetalingsdato, så `sumForskuddstrekkPerLoennsutbetalingsdato`
utelates; den har intet avgiftsgrunnlag, så `arbeidsgiveravgift` på
virksomheten utelates og `sumArbeidsgiveravgift` er 0. Alt er valgfritt
i XSD-en, og meldingen valideres mot den i enhetstesten.

Uten noen å rapportere (ingen ansatt i måneden) er det en hindring:
uten ansatte og uten utbetalinger skal det ikke leveres a-melding.

**En måned som har en kjøring, er kjøringens.** Kjøringens melding tar
allerede med arbeidsforholdene uten lønn, så en NY månedsmelding nektes
da. Får måneden en kjøring ETTER at månedsmeldingen er levert,
**erstatter** kjøringens første melding den (`erstatterMeldingsId`):
sendt ved siden av ville den rapportert de samme arbeidsforholdene to
ganger, siden meldinger for samme måned legges sammen hos Skatteetaten.

**En månedsmelding som feilet, blir ikke stående.** Et tidsavbrudd kan
komme ETTER at Skatteetaten tok imot meldingen, så et lagret, ikke
levert forsøk har ukjent skjebne. Det kan derfor fortsatt sendes på nytt
— samme kropp, samme nøkkel, telles ikke dobbelt — også etter at måneden
har fått en kjøring, og kjøringens første melding **nektes** så lenge et
slikt forsøk står: den kan ikke erstatte en melding uten kvittering, og
sendt ved siden av kunne den telt arbeidsforholdene to ganger. Når
forsøket er levert, erstatter kjøringens melding det som over.
Innsendingssporet er det samme som for en kjøring — lagret før det
sendes, samme kropp og nøkkel ved nytt forsøk, erstatning bare på
uttrykkelig forespørsel — men en månedsinnsending har ingen
`run_id` (migration 0081) og kjennes på selskap og måned; låsen er
selskapsraden i stedet for kjøringsraden.

`GET /companies/{id}/payroll/months/{ÅÅÅÅ-MM}/amelding` (`LONN_KJOR`)
gir forhåndsvisningen og `?format=xml` filen, med `…/submissions` og
`…/submissions/{sid}/feedback` som for en kjøring. I portalen: «A-melding
for en måned uten lønnskjøring» under kjøringene.

**Det regnmed ikke gjør**: å huske måneden for deg. Ingen jobb lager
eller sender månedsmeldingen av seg selv; lønnsansvarlig må åpne den,
som en kjøring.

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
- ansatte uten lønn i kjøringen hvis arbeidsforhold mangler felt — de
  er med i meldingen likevel (F108, se over), så feltene må finnes;
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

**En melding kan være `mottatt` mens måneden har avvik.** Skatteetatens
svar dekker hele måneder, og ett svar kan dekke flere. Da oktober ble
levert etter november i testmiljøet 2026-11-23, sto oktober uten avvik,
mens samme svar la `MAGNET_EDAG-108` på november fordi et arbeidsforhold
oktober rapporterte manglet der. Novembers egen leveranse sa fortsatt
`mottatt` uten avvik. `GET …/payroll` og `GET …/submissions` bærer derfor
`amelding_maaneder`: alle avvik i `mottattPeriode` per kalendermåned, fra
svaret på den NYESTE innsendingen som dekker måneden. Et gammelt svar som
hentes sent, overstyrer altså ikke et nyere. Portalen varsler øverst i
lønnskjøringene og viser avvikene i måneden til en erstatning har fjernet
dem. Testen
`an_avvik_on_another_month_is_shown_until_a_replacement_clears_it` fester
dette.

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
   og avgift i hele kroner og med arbeidsforholdene uten lønn i
   måneden (F108), også for en måned helt uten lønnskjøring.
   Innsendingen er bygget og prøvd i testmiljøet, men venter i
   produksjon på Altinn-systembrukeren (#180, docs/gov.md). Inntil
   den er på plass er dette internt regnskap, ikke rapportering —
   **selskapet må fortsatt levere a-melding på annet vis.** Dette er
   den viktigste mangelen.
2. **Trekkpålegg hentet fra Skatteetaten** (#334) er bygget
   (docs/trekkpalegg.md §2), men ikke prøvd mot testmiljøet: scopet
   venter (SSV-6690). Til det er tildelt, trekkes bare pålegg
   registrert for hånd (#339).
3. **Inntektsmelding til Nav** (#184). Når Nav ber om inntektsmelding
   for en sykmeldt, må selskapet sende den selv, på nav.no eller i et
   annet system. regnmed registrerer ikke fravær. Navs API tar bare
   imot svar på en forespørsel fra Nav. Forslaget står i docs/inntektsmelding.md.
4. **Sykepengerefusjon, naturalytelser, pensjonstrekk, tariff-logikk,
   OTP** — uttrykkelig utenfor v1 i #46 selv.

## Trekkpålegg (migration 0110, #339)

Et pålegg om utleggstrekk som kommer i brev, registreres for hånd under
Lønn → Trekkpålegg og trekkes i lønnskjøringen. Hele beskrivelsen —
de tre typene, feltene, attestering, bokføring og hva som er utsatt —
står i docs/trekkpalegg.md §6. Det lønnen gjør:

- **Trekket kommer etter forskuddstrekket** og regnes per ansatt med
  `regnmed-core::garnishment`: et beløp trekkes som det står, en
  prosent tas av grunnlaget for typen (samordnet og skatt holdes
  adskilt i koden, selv om de er like så lenge lønnen ikke har
  naturalytelser), og trekket rundes ned til hele kroner. Taket er
  netto minus trekkfri kjøregodtgjørelse og diett; ber påleggene om
  mer, trekkes det som rekker, i rekkefølgen samordnet, skatt,
  øvrige, og kjøringen advarer med begge beløpene.
- **Bokføringen** er i samme bilag som lønnen: netto på 2930 er
  redusert med trekket, og trekket står som leverandørpost på kontoen
  for utleggstrekk (2610 som standard) med KID og forfall i
  `payable_terms`, så betalingslisten betaler det.
- **Forhåndsvisningen** returnerer settet av pålegg (`trekkpalegg_sett`)
  den brukte, og bokføringen må sende det tilbake: er et pålegg
  registrert, endret, stoppet eller attestert etter
  forhåndsvisningen, nekter kjøringen.
- **A-meldingen** får `utleggstrekk` per ansatt (samordnet og skatt,
  negativt beløp, utbetalingsdatoen) og `sumUtleggstrekk`; øvrige krav
  rapporteres ikke.
- **Lønnsslippen** viser hvert trekk med type, KID og versjon.

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
av lønnslinjen — og linjene er innsettings-bare. Slippens egne tall står
derfor fast, så den rendres på forespørsel i stedet for å lagre enda en
kopi av personopplysninger. Hittil-i-år-tallene summeres fra kjøringene
hver gang, så de endrer seg hvis en kjøring bokføres senere med en
tidligere utbetalingsdato.

Slippen **forklarer** trekket i stedet for bare å oppgi det: grunnlaget
står i parentes («Forskuddstrekk (35 % av 55 000,00)»), og både
feriepengenes trekkfrihet og desembers halve trekk får sin egen linje
når de gjelder. Feriepenger opptjent denne måneden står under en egen
overskrift, tydelig utenfor «Til utbetaling», og hittil-i-år-tallene
står nederst.

**Hittil i år følger utbetalingsåret**, for brutto, forskuddstrekk og
opptjente feriepenger. Feriepengegrunnlaget teller lønn det året den
utbetales (ferieloven § 10 nr. 1), og trekktabellene hentes fra
utbetalingsdatoen. Desemberlønn utbetalt i januar står derfor under
«Hittil i 2027», selv om lønnsperioden er desember 2026. Kjøringene
telles i rekkefølge etter utbetalingsdato og deretter lønnsperiode, så
desember kommer foran januar også når begge utbetales samme dag.

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

Lønn-seksjonen starter med den månedlige jobben og har fire kort i
denne rekkefølgen: **Kjør lønn**, **Lønnskjøringer og a-melding**,
**Ansatte** og **Skattekort**.

**Kjør lønn** har år, måned, utbetalingsdato og sone, og én rad per
aktiv ansatt der brutto kan overstyres og utbetalte feriepenger legges
inn. Måneden er forhåndsvalgt til måneden etter siste kjøring (eller
inneværende måned når ingen er kjørt), og året kan settes til i fjor,
så desemberlønnen kan kjøres i januar. Allerede kjørte måneder er
deaktivert i velgeren, så den vanligste feilen ikke kan forsøkes.

**Sonen velges uttrykkelig ved selskapets første kjøring.** Portalen
forhåndsvelger ingen sone, og forhåndsvisningen er sperret til noen har
valgt en. Senere kjøringer arver sonen fra siste kjøring. Grunnen er at
en forhåndsvalgt sone I (14,1 %) ville gitt for høy arbeidsgiveravgift
for en virksomhet i for eksempel sone IV (5,1 %), og feilen ville fulgt
med inn i det bokførte bilaget og a-meldingen, der den bare kan rettes
med et reverserende bilag og en ny kjøring. API-et har heller ingen
standardsone: mangler `sone`, svarer det 422, og en tom eller ukjent
sone gir 400. Regnmed slår ikke opp sonen fra kommunen. Et slikt forslag
ville kreve et datert og kildebelagt register, slik satsregisteret er.

**Ingenting bokføres uten forhåndsvisning.** «Se hva som bokføres»
sender skjemaet til `POST …/payroll?preview=true`, som regner ut
kjøringen nøyaktig som den bokføres — samme satser, samme trekk, samme
feriepenge- og aga-avsetning, samme advarsler — men uten bilag, uten
bilagsnummer og uten kjøringsrad (`preview_payroll` i
`regnmed-db::payroll`; `id` og `voucher_id` er null-UUID). Den nekter
også en måned som alt er kjørt. Forhåndsvisningen viser per ansatt
brutto, feriepenger, trekk, kjøregodtgjørelse og netto, og i sum netto
til utbetaling, arbeidsgiveravgift, feriepengeavsetning, aga på
feriepenger og lønnskostnad. Bokfør-knappen («Bokfør lønn for juli
2026») finnes bare så lenge forhåndsvisningen svarer til skjemaet; en
endring i et felt tar den bort. Testen
`a_preview_books_nothing_and_matches_the_run` fester at
forhåndsvisningen ikke skriver noe og at summene er de samme som i den
bokførte kjøringen. Etter bokføringen står resultatet og eventuelle
advarsler på skjermen, med lenke til bilaget, og skjemaet går videre
til neste måned.

Ansatte med timesats og uten månedslønn starter med «Fra låste timer»
krysset av; serveren nekter hvis måneden ikke er låst.

Skjemaet viser alle som var ansatt i den valgte måneden, også etter
sluttdatoen, slik at et sluttoppgjør kan kjøres. Den som alt har sluttet,
står uten hake og med 0 kr i brutto, så en vanlig kjøring aldri betaler
vedkommende ved et uhell. Det som er skrevet inn, beholdes når måneden endres.

**Lønnskjøringer og a-melding** viser hver kjøring med brutto,
**utbetalte feriepenger**, trekk, kjøregodtgjørelse, netto og
avsetningene — uten feriepengekolonnen gikk ikke tallene opp når
feriepengene ble utbetalt. Hver kjøring har en knapp per ansatt (fullt
navn) som laster ned lønnsslippen som `lonnsslipp-ÅÅÅÅ-MM-navn.pdf`.

**Ansatte**-kortet har en **Portalbruker**-kolonne (koblet / invitert /
ikke koblet) med invitasjons- og koblingsflyten fra 0050; skjemaet for
ny ansatt tar e-postadresse rett i registreringen, så invitasjonen går
ut i samme håndvending. Å koble fra en portalbruker eller trekke
tilbake en invitasjon spør først, siden koblingen avgjør hvem som leser
lønnsslippen.

Verifisert i nettleser: registeret viser fødselsdato og ikke
fødselsnummer, juni (allerede kjørt) er deaktivert, og en kjøring
utført gjennom UI-et ga et bilag som summerer til nøyaktig null.

## Testet

- Trekkpålegg (`crates/regnmed-db/tests/payroll.rs`, modulen
  `garnishment`, ekte Postgres): et samordnet pålegg trekkes, bokføres
  på 2610 med KID og forfall, står i betalingslisten med brevets konto,
  rapporteres i en a-melding som valideres mot XSD-en og vises på
  lønnsslippen; en booking uten settet eller med et gammelt sett
  nektes; versjoner kan verken endres eller slettes i databasen; med
  attesteringspolicy venter registreringen på en annen person mens en
  attestert versjon fortsetter; skattetrekk blir én samlet post med
  selskapets KID og nektes uten innstillingene; øvrige krav får brevets
  frist og rapporteres ikke; taket og 2611-reserven. Vaktene for settet
  og attesteringen er sett feile. Regnereglene og valideringen har
  enhetstester i `regnmed-core::garnishment`, a-meldingsfeltene i
  `regnmed-core::amelding`.

- Endret feriepengesats (`crates/regnmed-db/tests/payroll.rs`, ekte
  Postgres): at kjøringen advarer når tillegget mangler, at en hevet sats
  tar igjen tillegget nøyaktig én gang også når to kjøringer går samtidig,
  at året ender på 10,2 % + 2,3 % av 6 G, at avtalte satser ikke bruker opp
  taket, at gamle lønnsslipper beholder satsen sin, og at endringen holder
  seg innenfor selskapet. Tilgangsvakten er festet i `grupper/access.rs`.
- Trekk i feriepenger utenfor fritaket: enhetstester i `regnmed-core::payroll`
  for fratreden, grensen på 21 % (også utbetalt i to deler), åpningsbalanser,
  at årets feriepenger ikke trekkes to ganger, kortets prosent på tabellkort
  og fullt trekk i desember. Mot ekte Postgres (`regnmed-db/tests/payroll.rs`):
  en hel fratreden med fjorårets og årets feriepenger, avtalt sats over
  grensen, avhukingen for dødsfall, og tabellkort med og uten lagret prosent.
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
  desember), trekkfrie feriepenger, trekk i 60-årstillegget (også over 6 G,
  på tariff, i tabell og i desember), at en delutbetaling nekter til
  tillegget er oppgitt, halv skatt i desember, kort over 12
  måneder (trekk i feriepenger, fullt i desember), frikort med grense
  (under, over og på vei over), trekk og arbeidsgiveravgift avrundet
  ned til hele kroner (Skatteetatens 1 727,25 → 1 727, og ned også ved
  ,99), og at ukjente tabeller, år og sone Ia nektes med begrunnelse.
- Mot ekte Postgres: 60-årstillegget fra opptjening til utbetaling (regnet
  ut for hele fjorårets opptjening, nektet og oppgitt ved delutbetaling, én
  `feriepenger`-linje i a-meldingen).
- Mot ekte Postgres: frikort over to kjøringer (januar under grensen,
  februar 50 % av det som overstiger) med fjorårets lønn utenfor
  tellingen, at en kjøring utbetalt i et år uten hentet frikort nekter,
  et 12-månederskort med feriepenger, at lønnsslippen bærer metoden
  fra kjøringen, og at to lønninger med øre gir avgift regnet én gang på
  summen (10 481 kr, ikke 10 480 per ansatt) som a-meldingen godtar;
  at ansatte uten lønn er med i meldingen med arbeidsforholdet alene, i
  UTBETALINGSmånedens kalender (marslønn utbetalt i april tar med den
  som begynte i april, ikke den som sluttet i mars), uten å endre
  avgiften; og at sluttdatoen nektes før startdatoen, kan fjernes og
  ikke når et annet selskaps ansatt; og at en sluttdato ført etter at
  februars melding var levert uten den, gir mars-melding MED
  sluttdatoen og en april-melding uten den ansatte, mens den som sluttet
  før februar holdes ute. Sett feile med lønnsmåneden i stedet for
  utbetalingsmåneden, med de ulønnede utelatt og med regelen for sen
  sluttdato fjernet. For en måned uten kjøring: at meldingen bærer
  hvert arbeidsforhold alene og validerer mot XSD-en uten
  utbetalingsdato og avgiftsblokk, at en tom måned er en hindring, at
  den lagres, prøves på nytt uendret og leveres én gang, at den nektes
  når måneden har en kjøring, og at kjøringens første melding da
  erstatter den (men ikke i en måned uten levert månedsmelding); og at
  en månedsmelding som feilet, fortsatt kan sendes på nytt etter at
  måneden fikk en kjøring, mens kjøringen nektes til den er levert — sett
  feile med erstatningen, kjøringsvakten, ventevakten og den gamle
  nektingen av nye forsøk. Gjennom API-et:
  forhåndsvisning, ugyldig måned, nektet måned med kjøring, innsending,
  liste og tilbakemelding mot en etterlignet Skatteetaten.
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
  fødselsnummer. Desember: at bonus bare halveres opp til gjennomsnittet,
  at en feriepengemåned ikke trekker snittet ned, at timelønn og en ansatt
  uten tidligere lønn trekkes fullt med advarsel, og at samme utbetalingsår
  ikke får to halve trekk.
- Timelønn: at beløpet regnes fra minutter, at en ULÅST måned nektes,
  at kjøringen går etter låsing og gir riktig brutto og trekk, og at en
  ansatt uten portalbrukerkobling gir en tydelig feil framfor null timer.
- `regnmed-core::amelding` — en månedsmelding validert mot den
  offisielle XSD-en med xmllint (sett feile på et feilstavet element),
  også en 2025-melding med én trekksum, en erstatningsmelding og en
  mottaker med arbeidsforholdet alene (F108); at
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
  server (2,8 kB, uten fødselsnummer). Desemberlønn utbetalt i januar
  teller i det nye året og foran januarlønnen.
