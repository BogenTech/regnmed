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
   sted, hos oss.
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

## 6. Driften

Det finnes ingen API-vei for å styre abonnementer — det er driftens
jobb, som migrate og anchor, og en plattformadministrator finnes ikke
(docs/auth.md §8):

```sh
regnmed abonnement --orgnr 999888777 --aksjon tegn --note "Avtale 2026-014"
regnmed abonnement --orgnr 999888777 --aksjon avslutt --til 2026-09-01
regnmed abonnement-faktura --orgnr <driftsselskapets orgnr>          # hele måneden
regnmed abonnement-faktura --orgnr <drift> --bare-orgnr 999888777    # etterfakturering
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
