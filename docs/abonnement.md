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

**Betalingsleverandør er med vilje ikke valgt i v1**, for faktura+KID
trenger ingen: null transaksjonsgebyr, null PCI-omfang, null
hemmeligheter, og B2B-kunder aksepterer faktura. Når tier 2 bygges er
retningen **kort først** (vurdert i #65, bekreftet 2026-07-28):
bedrifter betaler SaaS-abonnementer med firmakort, og for gjentagende
kortbetaling er **Stripe** den sterkeste plattformen (Billing, SCA,
automatisk kortfornyelse, dunning) — redirect-/Checkout-basert, så
kortdata aldri berører oss. Vipps MobilePay er en mulig
TILLEGGS-skinne for ENK/småbedrifter senere, ikke hovedvalget.
PSP-nøkler er hemmeligheter og bor utenfor repoet (docs/secrets.md);
webhook-mottak må være idempotent. Ingen av delene endrer modellen her
— en PSP er bare en raskere vei til «betalt» på samme reskontropost.

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
- `crates/regnmed-api/tests/abonnement.rs`: prøvetiden virker; sperret
  selskap nektes endringer med forklaring **og bevises å kunne lese,
  eksportere SAF-T og administrere tilgang**; tegning åpner igjen;
  oppsigelse gir frist, ikke øyeblikkelig sperre; fakturakjøringen
  utsteder gjennom egen motor, med prislistens beløp mot kundens orgnr,
  og samme måned kan ikke faktureres to ganger.
