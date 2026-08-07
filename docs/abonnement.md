# Abonnement: regnmeds egen kasse

Fram til #65 var regnmed gratis for alle, for alltid — ikke som
beslutning, men fordi ingen hadde bygget noe annet. Dette dokumentet er
forretningssiden: hva et abonnement er, hva det koster, hvordan det
faktureres, og hva som skjer når det ikke betales.

## 1. Prinsippet som ikke er forhandlingsbart

**Hovedboken tas aldri som gissel.** Bokføringsloven pålegger kunden
oppbevaring, og hele tillitshistorien er at regnskapet kan verifiseres
og tas med ut. Derfor:

- Et utløpt abonnement sperrer **endringer** — som en låst periode.
- **Lesing virker alltid.** Rapporter, spesifikasjoner, nedlastinger.
- **Eksport virker alltid.** SAF-T-eksporten er veien ut, og den er
  åpen nettopp når kunden er misfornøyd nok til å ville dra.
- **Styringen av selskapet virker alltid**: tilgang, oppdrag,
  integrasjoner og firmaopplysninger. Et sperret selskap må kunne
  slippe inn den som skal ordne opp, og avvikle det som skal avvikles.
- **Å dokumentere det som alt er bokført virker alltid** (#85).
  `VEDLEGG_SKRIV` står på den åpne siden: bokføringsloven §10 krever at
  bokførte opplysninger er dokumentert, og dokumentasjon kommer
  legitimt i etterkant. En manglende betaling kan ikke hindre den
  bokføringspliktige i å oppfylle en lovpålagt plikt på poster som
  allerede står — et vedlegg fører ingenting nytt inn i regnskapet.
  docs/perioder.md trekker samme grense for LÅSTE perioder.
  `BILAG_LAST_OPP` (innboksen) er bevisst fortsatt sperret: der kommer
  det inn NYE dokumenter som skal bokføres.

Sperren håndheves i tilgangsvakten (docs/auth.md §3) — én søm, ingen
endepunkter kan glemme den. Skillet mellom «endrer» og «leser» er en
uttømmende match i koden ([`Rett::endrer`]), så en ny rettighet tvinges
til å velge side.

## 2. Statusene

Beregnet, aldri lagret (`regnmed-core::abonnement`, ren logikk med
enhetstester):

| Status | Når | Virkning |
| --- | --- | --- |
| `prove` | nytt selskap, ingen dekning, under 30 dager gammelt | alt virker; banner sier når prøvetiden løper ut |
| `aktiv` | en abonnementsrad dekker dagens dato | alt virker |
| `frist` | dekningen (eller prøvetiden) er ute, mindre enn 14 dager siden | alt virker; banner varsler sperredatoen |
| `sperret` | fristen er ute | endringer avvises med forklaring; lesing/eksport/styring virker |

Grensene er eksklusive som overalt ellers (`valid_to` er dagen
dekningen *ikke lenger* gjelder). Fristen regnes fra det seneste av
prøvetidens slutt og siste deknings slutt — et selskap som sier opp
etter to år får 14 dagers frist fra oppsigelsen, ikke fra en prøvetid
som løp ut for lengst.

## 3. Dataene

Migrasjon 0041, tre tabeller med kjente mønstre:

- **`abonnement`** — daterte dekningsrader per selskap
  (mva_terminordning-mønsteret). Hver rad bærer `note` med avtale-/
  vedtaksreferansen. Applikasjonsrollen kan tegne (insert) og avslutte
  (update av `valid_to` alene); ingenting slettes. Selskaper som fantes
  ved innføringen fikk en åpen dekning — pilotkunder sperres ikke av en
  migrasjon.
- **`abonnement_pris`** — prislisten som data, datert, insert-only, med
  kilde per rad (satsregister-mønsteret). Fakturaen bruker prisen som
  gjaldt på faktureringsdagen.
- **`abonnement_faktura_run`** — insert-only kjøringslogg, unik per
  (selskap, år, måned), skrevet i samme transaksjon som fakturaen: en
  måned kan ikke faktureres to ganger, og en feilet fakturering
  etterlater ingen rad og prøves igjen.

## 4. Prisen (vedtatt 2026-07-28, migrasjon 0042)

Markedet priser **modulene**: konkurrentene annonserer ~200 kr/mnd og
tar 400–700 når kunden trenger bank, KID, lønn og timer (verifisert
mot prissidene 2026-07-28 — Tripletex «fra 199», men lønn krever Pro
479 + 65/lønnsbruker; Fiken 219 + 69 per modul per bruker; Conta
209/339 årlig fakturert, lønn som tillegg; PowerOffice 425 + 9 kr per
bilag). regnmed angriper strukturen: **alt er inkludert, brukere
koster aldri, ingen bilagsgrenser** — og det eneste som prises er det
eneste som faktisk koster noe per kunde, menneskelig support:

| Plan | Pris (eks. mva, per selskap) | Forskjellen |
| --- | --- | --- |
| `basis` | **49 kr/mnd** | alt inkludert, selvbetjent (dokumentasjonen er supporten) |
| `standard` | **99 kr/mnd** | alt inkludert + e-postsupport |

**Mva legges på etter driftsselskapets EGEN registrering**, aldri som
en konstant (`abonnement::utgaende_mva`). Prisene over er eks. mva; er
driftsselskapet registrert i Merverdiavgiftsregisteret på fakturadatoen,
får linjen kode 3 og alminnelig sats fra satsregisteret. Er det ikke
registrert, får linjen kode 7 «Ingen mva-behandling (inntekter)» og
ingen avgift — mval. §11-4 forbyr å oppgi merverdiavgift i
salgsdokumentet når selgeren ikke er registrert, og et beløp som likevel
oppgis skal innbetales til staten. Koden var hardkodet `3` fram til
2026-08-07, skrevet den gangen driftsselskapet tilfeldigvis var
registrert; registreringsstatusen er datert stamdata (#81), og det er
nettopp det som gjør den lesbar per fakturadato. Kode 7 og ikke 6:
«omsetning utenfor merverdiavgiftsloven» er en påstand om VIRKSOMHETEN,
og å selge regnskapsprogram ligger godt innenfor loven — selgeren er
bare under grensen. Det samme oppslaget bestemmer bruttoen Stripe-Prisen
opprettes med, ellers ville kortet blitt trukket 25 % mer enn fakturaen
viser og beløpskontrollen i `bokfor_stripe_betaling` slått ut ved hvert
trekk.

Funksjonelt er planene identiske — skillet håndheves av
supportkanalen (et menneske som ser på planen), ikke av koden, og skal
forbli slik: en funksjonssperre ville gjeninnført modulmazen vi
konkurrerer mot. En prisendring er en ny rad i `abonnement_pris` med
kilde; eksisterende kunder beholder sin plan — grandfathering er
gratis når prisen er daterte data.

Bevisst utsatt, med retning: **byråavtaler** (byrået betaler rabattert
for klientene — regnskapsførerne er distribusjonskanalen, og modellen
her må formes med de første pilotbyråene, ikke gjettes); gratisplan
for sovende selskaper (en rad med pris 0 fakturerer ingenting —
mekanismen finnes alt).

## 5. Faktureringen: egen motor, ingen betalingsleverandør

Abonnementsfakturaene utstedes av **regnmeds egen fakturamotor** i
driftsselskapets hovedbok: gap-frie nummer, KID, reskontro, purring —
alt som gjelder kundene gjelder oss. `regnmed abonnement-faktura`
(månedlig CronJob i prod-overlayet; driftsselskapet pekes ut med
`REGNMED_DRIFT_ORGNR`) fakturerer alle selskaper med dekning den 1. i
måneden; kundeparten opprettes fra selskapets orgnr ved første kjøring.
Innbetalingen kommer inn som alle andre — OCR/bank på KID — og lukker
reskontroposten. Purremaskineriet (#29) håndterer resten.

### 5.1 To bruk av Stripe som ikke må forveksles

Setningen «aldri Stripe Billing» over gjelder **vår fakturamotor** —
fakturaene kundene våre sender til *sine* kunder. Den er produktet, og
den skal aldri outsources.

**Abonnementet kundene betaler OSS er en annen sak.** Der er Stripe
leverandøren, og fra 2026-07-30 brukes deres **Subscriptions**: trekket
gjentar seg til noen sier opp, Stripe eier gjentakelsen og
purre-/nyforsøksklokka, og kortdata når oss aldri — vi har ingen ønske
om å være PCI-compliant, og da skal kortet ligge hos en som er det.

Det som fortsatt er VÅRT, og hvorfor:

| Ting | Hvor | Hvorfor ikke hos Stripe |
| --- | --- | --- |
| Dekningsradene (`abonnement`) | Postgres | Sperreregelen i §1 er regnmed-oppførsel; ingen betalingsleverandør vet at lesing og eksport alltid skal virke |
| Prøvetid/frist/sperret | `regnmed-core::abonnement` | Ren, enhetstestet logikk — beregnet, aldri lagret |
| Prislisten (`abonnement_pris`) | Postgres | Datert data med kilde, som satsregisteret. Stripe-prisene OPPRETTES fra den |
| Bokføringen | Driftsselskapets hovedbok | Bokføringsloven bryr seg ikke om hvem som krevde inn pengene |

**To skinner går side om side med vilje.** Selskaper som alt hadde
dekning (migrasjon 0041 ga alle en) fortsetter på månedsjobben med
faktura og KID; bare NYE tegninger går via Stripe. Ingen pilotkunde
tvinges til å taste kort på nytt for at vi skal bytte mekanikk.
Månedsjobben hopper over selskaper med et løpende Stripe-abonnement —
og det er en eksplisitt utelukkelse i spørringen, ikke bare
kjøringsloggen: jobben går den 1., Stripe trekker på abonnementets egen
dato, så loggen ville oppdaget dobbeltfaktureringen først etterpå.

Flyten (migrasjon 0045, alle ledd idempotente):

1. Admin velger plan og intervall i portalen → Stripe Checkout i
   **abonnementsmodus**. Kortet lagres og gjentakelsen starter i ett
   steg.
2. `customer.subscription.created` åpner dekningen. Det er
   betalingsleverandørens bekreftelse som gjør tegningen ekte, ikke vårt
   eget klikk.
3. `invoice.paid` bokfører **vår vei**: faktura gjennom den ordinære
   motoren (gap-frie nummer, KID, reskontro), betalingsbilag mot den, og
   reskontroposten lukket — alt i ÉN transaksjon. Fakturaen er «betalt»
   ved konstruksjon, ikke ved et flagg: fordringen er fullt matchet i det
   øyeblikket den finnes. Beløpet fra Stripe er BRUTTO og splittes med
   `split_gross` på satsen som gjaldt betalingsdagen — mva beregnes ett
   sted, hos oss (og satsen er driftsselskapets egen, se kapittel 4: et
   uregistrert driftsselskap splitter på 0, altså er hele trekket
   grunnlaget).
4. `invoice.payment_failed` **logges bare**. Stripe forsøker igjen, og
   dekningen står åpen imens: å sperre på ett feilet trekk ville tatt
   hovedboken som gissel over et kort som kanskje går i morgen.
5. `customer.subscription.deleted` lukker dekningen.

**Oppsigelsen er selvbetjent** (`POST …/subscription/cancel`,
`SELSKAP_ADMIN`) og skjer **ved periodeslutt** — kunden beholder det som
er betalt for. Umiddelbar oppsigelse ville tatt bort tilgang som alt er
kjøpt, som er det motsatte av §1.

Stripe Tax er bevisst AV: vi kan den norske satsen fra satsregisteret,
og en avgift beregnet to steder er en avgift som før eller siden
spriker. Stripe-prisen er derfor brutto, og kunden ser hos Stripe
nøyaktig det de betaler.

### 5.2 Den opprinnelige kortskinnen

**Kort er standardveien fra dag én** (#74, besluttet 2026-07-28):
faktura+KID krever at noen *ser* innbetalingen — bankfiler importeres
manuelt til det finnes bank-API — mens et korttrekk bekreftes av en
webhook uten et menneske i nærheten. Leverandøren er **Stripe**
(sterkest på gjentagende kort: SCA, kortfornyelse; vurdert mot
Nets/Nexi Easy og norske Dintero, som begge priser ved avtale og er
reelle byttekandidater ved volum). Prinsippet som gjør byttet billig:
**vår fakturamotor er autoritativ** — aldri Stripe
Billing/Subscriptions; Stripe er bare en raskere vei til «betalt» på
samme reskontropost.

Flyten, alle ledd idempotente:

1. Admin legger inn kort i portalen: Stripe Checkout i **setup-modus**
   (hosted — kortdata berører oss aldri; vi lagrer referanser og
   brand/last4). Kortet kommer tilbake via webhook.
2. Selvbetjent tegning: admin velger plan og starter abonnementet —
   kort-først; uten kort avtales faktura med drift.
3. Månedlig kjøring utsteder fakturaen som før, og trekker kortet
   off-session med **fakturaens id som idempotensnøkkel** — samme
   faktura kan aldri trekkes to ganger, uansett omkjøringer.
4. Webhooken (signaturverifisert, HMAC-SHA256 hand-rolled og testet mot
   RFC 4231) bokfører betalingsbilaget (1570 Kortoppgjør mot 1500 med
   part) og lukker reskontroposten i ÉN transaksjon med loggraden i
   `kortbetaling`; unikheten på payment_intent gjør replay til en
   no-op. Feilede trekk logges — purring/sperre tar oppfølgingen (#75).
5. Stripe-utbetalingen til driftskontoen avstemmes i den ordinære
   bankmotoren (1570 → 1920, gebyret kostnadsføres).

Konfig: `STRIPE_SECRET_KEY` + `STRIPE_WEBHOOK_SECRET` (begge eller
ingen; nøkler er hemmeligheter og bor utenfor repoet, docs/secrets.md)
+ `REGNMED_DRIFT_ORGNR` på API-et (webhooken må vite hvilken hovedbok
trekket hører hjemme i). Uten nøklene er kortskinnen AV og portalen
sier det. Vipps MobilePay er en mulig tilleggs-skinne senere;
merchant-of-record (Paddle o.l.) er avvist — de blir selger, og det
kolliderer med at vi fører vårt eget regnskap i vårt eget system.

### 5.3 Automatisk oppfølging (#75): send, purr, sperr — uten mennesker

«Sending er menneskelig handling» gjelder KUNDENES bokføring;
driftsselskapets egen fakturering er nettopp maskinell. Den daglige
CronJobben `regnmed abonnement-oppfolging` lukker livssyklusen:

1. **Send**: abonnementsfakturaer uten utsendelsesrad e-postes til
   kundeselskapets egen adresse (Firmaopplysninger) med PDF-en vedlagt.
   Utsendelses-id-en er Nats-Msg-Id, så en omsending kan aldri
   dobbeltlevere — og en faktura kortet betalte ved utstedelse har
   ingenting utestående og sendes ikke som krav.
2. **Purr**: kadensen er en REN REGEL (`regnmed-core::purring::
   neste_skritt`): gratis påminnelse 3 dager etter forfall, purring med
   gebyr (standardkompensasjon — kundene er næringsdrivende) 14 dager
   senere, inkassovarsel med rente 14 dager etter det — og så stopper
   maskinen: inkasso hører til bevillingshavere (docs/purring.md).
   Hvert skritt går gjennom samme `create_reminder` som menneskene
   bruker — stegreglene og satsregisteret håndhever lovligheten, kravet
   bokføres i samme transaksjon, dokumentet lagres. En purring må nå
   noen: selskap uten e-postadresse purres ikke, men rapporteres
   høylytt til noen retter adressen.
3. **Sperr**: står fakturaen ubetalt
   `SPERR_ETTER_FORFALL_DAGER` (30) dager etter forfall OG en purring
   er sendt i en TIDLIGERE kjøring, avsluttes dekningen. Det sperrer
   ikke i seg selv — den vanlige fristen på 14 dager løper oppå
   (statusen er fortsatt beregnet, aldri lagret), så selve sperren
   lander ~44 dager etter forfall. Kortabonnenter (Stripe
   Subscriptions) har sin egen vei: Stripe purrer selv, og
   `customer.subscription.deleted` avslutter dekningen som før.
4. **Gjenopprett**: når alle abonnementsfakturaer er betalt (webhook
   eller bankimport/manuell match), tegnes dekningen på nytt med samme
   plan — ØYEBLIKKELIG fra kortwebhooken, ellers ved neste kjøring.

Beslutningene om dekning står i det innsettings-bare sporet
`abonnement_oppfolging` (migrasjon 0048) — som også er maskinens eget
minne: gjenoppretting skjer BARE for dekninger maskinen selv avsluttet
for mislighold. En oppsigelse ser identisk ut i abonnement-tabellen, og
uten sporet ville en betalt sluttfaktura vekket det oppsagte
abonnementet til live igjen. Gebyr- og rentekrav er egne åpne poster på
samme reskontro; gjenopprettingen krever fakturaene betalt, ikke
gebyrene — et gebyr holder aldri hovedboken som gissel.

Utsendingen bruker samme mail-skinne som alt annet; publisisten bor nå
i egen crate (`regnmed-mail`) siden BÅDE API-et og CLI-en sender, og en
wire-kontrakt skal ha nøyaktig én kopi.

## 6. Driften

Det finnes ingen API-vei for å styre abonnementer — det er driftens
jobb, som migrate og anchor; plattformrollene (docs/auth.md §8) når
stamdata, ikke abonnementer:

```sh
regnmed abonnement --orgnr 999888777 --aksjon tegn --note "Avtale 2026-014"
regnmed abonnement --orgnr 999888777 --aksjon avslutt --til 2026-09-01
regnmed abonnement-faktura --orgnr <driftsselskapets orgnr>          # hele måneden
regnmed abonnement-faktura --orgnr <drift> --bare-orgnr 999888777    # etterfakturering
regnmed abonnement-oppfolging --orgnr <drift>                        # daglig: send, purr, sperr (§5.3)
regnmed abonnement-pris                                              # vis prislisten
regnmed abonnement-pris --plan standard --pris-ore 12900 \
    --fra 2027-01-01 --kilde "prisvedtak 2026-12-01"                 # prisendring = én kommando
```

Kunden ser statusen sin i `/me` og som banner i portalen; banneret er
beskjeden, **serveren er sperren**.

## 7. Hvor det er testet

- `regnmed-core::abonnement`: statusovergangene, inkludert de
  eksklusive grensedagene og frist-fra-oppsigelse.
- `crates/regnmed-api/tests/grupper/abonnement.rs`: prøvetiden virker; sperret
  selskap nektes endringer med forklaring **og bevises å kunne lese,
  eksportere SAF-T og administrere tilgang**; tegning åpner igjen;
  oppsigelse gir frist, ikke øyeblikkelig sperre; fakturakjøringen
  utsteder gjennom egen motor, med prislistens beløp mot kundens orgnr,
  og samme måned kan ikke faktureres to ganger.
- Oppfølgingen (§5.3): `regnmed-core::purring` fester kadensen
  (påminnelse → purring → inkassovarsel → stopp) som ren regel;
  integrasjonstesten går hele trappen på flyttet klokke — gebyret på
  purringen, dekningen avsluttet på dag 30 og logget, inkassovarselet
  etterpå, stillhet etter det, gjenoppretting nøyaktig én gang når
  betalingen er matchet — og at en OPPSIGELSE aldri gjenopprettes.
