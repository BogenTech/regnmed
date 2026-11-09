# Bakgrunnsjobber

Arbeid som tar lenger tid enn en forespørsel bør holdes åpen, kjøres i
`regnmed work` i stedet for i API-et. Brukeren får svar med én gang
(`202` og en jobb-id), portalen viser fremdriften, og resten av
portalen er like rask mens jobben går.

**Bokføring er IKKE en bakgrunnsjobb.** Et bilag bokføres i én
transaksjon i forespørselen, og svaret er bilagsnummeret eller grunnen
til at det ble avvist. Det tar noen millisekunder og vokser ikke med
hovedboken (målt 2026-11-09: ~4,8 ms per bilag lokalt, likt fra første
til tjuetusende). Å legge det i kø ville gitt en ny feilform — «mottatt,
men ble det bokført?» — uten å gjøre noe raskere.

## Hva som kjører som jobb

| Jobb | Endepunkt | Rettighet | Målt (2026-11-09, release-bygg lokalt) |
|---|---|---|---|
| SAF-T-import | `POST /companies/{id}/jobs/saft-import` | `MIGRERING_ADMIN` | 10 429 bilag (10 MB) på 48 s |
| Revisjonsrapporten | `POST /companies/{id}/jobs/audit-report` | `RAPPORT_LES` | 20 000 bilag + 2 000 vedlegg på 1,1 s |

`GET /companies/{id}/jobs/{job_id}` gir status (`queued`, `running`,
`done`, `failed`) og deretter `result` eller `error`. Lesing krever samme
rettighet som å starte jobben: resultatet er like følsomt som det jobben
regnet ut. Uten tilgang til selskapet svarer endepunktet `404`, som
overalt ellers.

En SAF-T-fil som ikke er SAF-T i det hele tatt, avvises med `400` når
jobben opprettes (tolkingen tar under ett sekund). Feil som først viser
seg under importen — en åpningsbalanse som ikke stemmer, en fil som alt
er importert — blir en jobb med status `failed` og meldingen i `error`.

De synkrone endepunktene (`/import/saft`, `/reports/revisjon`) består for
integrasjoner som allerede bruker dem. Portalen bruker jobbene.

## Slik er det bygget, og hvorfor det skalerer

Samme mønster som agentkøen (docs/agent.md §8.7), som har kjørt i drift
siden oktober:

- **`job`-tabellen er fasiten** (migrasjon 0071). Hva som ble bestilt —
  type, selskap, input, hvem — er uforanderlig (kolonnerettigheter); bare
  status og fremdrift endres. Inputen (SAF-T-fila) lagres gjennom
  dokumentlageret som alle andre dokumenter (docs/blob-store.md).
- **NATS er en dørklokke.** API-et legger jobben i tabellen og ringer på
  `regnmed.jobs` (strømmen `REGNMED_JOBS`, work queue). Meldingen bærer
  bare jobb-id-en, aldri regnskapsdata.
- **Alle replikaene deler én varig konsument** (`jobworkers`), så
  JetStream gir hver melding til én av dem. Flere replikaer gir flere
  jobber samtidig.
- **Jobben gripes i databasen før noe skjer** (`regnmed_db::job::claim`,
  én atomisk `update`). En melding som leveres to ganger, et feiebrett
  som kjører samtidig, eller to replikaer som får samme melding — bare én
  vinner. Testet med 16 samtidige arbeidere over 150 jobber og ti
  samtidige grep om én jobb (`crates/regnmed-db/tests/job.rs`).
- **Hjerteslag hvert 15. sekund.** En arbeider som dør, slutter å slå;
  etter 60 sekunder kan en annen ta over jobben. En arbeider som ble
  antatt død og våkner, får ikke overskrive etterfølgerens resultat
  (utfallet skrives bare mens `claimed_by` fortsatt er den). Etter tre
  forsøk merkes jobben `failed` — en jobb som dreper arbeideren sin tre
  ganger trenger et menneske.
- **Hver replika feier hvert 30. sekund** etter jobber ingen melding nådde
  fram til (NATS nede, en melding tapt), med `for update skip locked`, så
  replikaer som feier samtidig aldri tar samme jobb. Uten `NATS_URL`
  feier den hvert 3. sekund og virker likevel.
- **Hver replika kjører `JOB_CONCURRENCY` jobber samtidig** (standard 2),
  og venter på ledig plass FØR den griper en jobb, så en travel replika
  aldri holder en jobb den ikke kan starte.
- **SIGTERM** stopper nye jobber og venter opptil 110 sekunder på dem som
  kjører (podens `terminationGracePeriodSeconds` er 120). En rullerende
  oppdatering avbryter altså ikke en import halvveis.

Logikken bor i `regnmed-db::job`, så arbeideren og API-et kjører samme
kode. Uten NATS (lokal utvikling, integrasjonstestene) kjører API-et
jobben selv i bakgrunnen; i clusterne er køen alltid konfigurert.

## Drift

Deployment `regnmed-worker` (`deploy/shared/job-worker.yaml`) kjører
`/app/regnmed work` fra samme image som API-et. Skaler med replikaer når
jobber blir liggende i kø:

```sh
kubectl -n regnmed scale deploy/regnmed-worker --replicas=3
```

Varige endringer gjøres i overlayet. Minnegrensen er 256 Mi: en SAF-T-import
holder fila og den tolkede formen mens den bokfører (målt 55 MB for en
fil på 10 MB).

`regnmed work --once` kjører det som venter og avslutter — nyttig for å
tømme køen for hånd.

## Ytelsesfunn som kom med (2026-11-09)

Målingene for dette arbeidet fant to feil som var verre enn ventetiden de
skulle flytte:

- **SAF-T-tolkingen var kvadratisk.** Linjenummeret til feilmeldinger ble
  regnet ved å telle linjeskift fra starten av fila ved HVER XML-hendelse.
  En fil på 10 MB tok over ti minutter bare å tolke; nå tar den 0,15 s.
  Festet av `parsing_time_grows_linearly_with_the_file`, som ble sett feile
  på den gamle koden.
- **Kjedeverifiseringen gjorde én spørring per bilag** og holdt hele
  hovedboken i minnet. Nå går den i sider på 1 000 bilag med én spørring
  per side, og vedleggene sjekkes åtte om gangen uten at alle bytene
  holdes samtidig. Revisjonsrapporten for 20 000 bilag: 11,5 s → 1,0 s.
  `verify-ledger` og backup-verifiseringen får samme gevinst.
