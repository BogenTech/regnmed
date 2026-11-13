# Bilagstolkning: forslag fra dokumentets egen tekst

Issue #34. Målet er å spare regnskapsføreren tastetrykk per
innboksdokument — **uten** at en maskin noen gang bokfører.

## Den harde regelen først

Det finnes ingen automatisk bokføringsvei. Ikke «over beløpsgrense
X», ikke «når vi er sikre nok». Forslaget fyller ut det eksisterende
bokfør-skjemaet i Bilag-seksjonen; mennesket ser tallene, retter det
som er galt, og bokfører. Det posterte bilaget er fortsatt det eneste
med rettslig betydning (docs/bilagsinnboks.md), og med aktiv
attestering gjelder fire øyne som før (docs/attestering.md).

I portalen er forslaget **merket som forslag**: kilden står over
skjemaet, sammen med begrunnelsen for hvert felt.

## Tre kilder, i rekkefølge

`GET /companies/{id}/inbox/{doc}/forslag` svarer alltid, og sier hvor
tallene kommer fra:

| `kilde` | Når | Presisjon |
| --- | --- | --- |
| `ehf` | Dokumentet er EHF/UBL (docs/ehf.md) | Eksakt — feltene står i filen |
| `pdf-tekst` | PDF med tekstlag | Heuristisk, med begrunnelse per felt |
| `tekst` | Ren tekstfil | Samme heuristikk |
| `ocr` | Foto (JPEG/PNG) lest av tekstleseren, når den kjører | Samme heuristikk, over OCR-tekst |
| `ingen` | Alt annet, eller et foto uten tekstleser | Ingenting foreslås, og det sies fra |

Et dokument uten lesbar tekst gir altså **ingen** felter — ikke
gjetninger.

## Tekstleseren (OCR) — en valgfri sidecar

`crates/regnmed-ocr` (levert 2026-11-13) er en egen liten tjeneste:
`POST /read` med et bilde inn, `{"text": …}` ut. Ingen database, ingen
hemmeligheter, ingen ingress — bare `regnmed-api` når den, gjennom
`OCR_URL`. Uten `OCR_URL`, eller når tjenesten ikke svarer, får et foto
ingen forslag og en advarsel; skjemaet virker som før, for hånd.

**Hvorfor en egen prosess.** Modellene bruker 250–310 MB mens de leser
(målt på macOS for bilder på 800–2400 px). Det har verken API-et
(64 MB) eller jobbarbeideren (256 MB). Tjenesten leser ett bilde om
gangen, og `RTEN_NUM_THREADS=2` begrenser CPU-en den tar fra noden.

**Motoren er `ocrs`** — ren Rust, så binæren er statisk som resten av
clusteret. Modellene (12 MB) er data, ikke kode: de hentes og
sjekksummeres av `scripts/fetch-ocr-models.sh` og kopieres inn i
imaget. En ny modell er en commit med ny sjekksum, aldri en
overraskelse — den endrer hva hver kvittering foreslår.

**Ærlig begrensning: modellen kan bare ASCII.** «Ø» blir «O», «ø» blir
«?». Det koster lite der det betyr noe, fordi heuristikken matcher på
normaliserte nøkkelord og på sifre. Det koster butikknavnet bokstavene
sine — derfor slås navnet opp i Enhetsregisteret på orgnr når det
finnes.

**Leserekkefølgen er vår.** `ocrs` grupperer tekst i blokker, så en
kvitterings «TOTALT» og «157,70» kommer ut langt fra hverandre — mens
heuristikken finner et beløp på etiketten på samme linje. `rows()` i
`regnmed-ocr` setter hver trykte linje sammen igjen: to biter deler
linje når de overlapper vertikalt med minst halvparten av den laveste,
målt mot linjens FØRSTE bit (en voksende union sluker neste linje på en
tettrykt kvittering — testet).

Bildet roteres etter kameraets EXIF-merking og skaleres ned til 2400 px
på lengste kant før lesing.

## Tekstlaget ut av PDF-en

`regnmed-core::pdftext` leser PDF-ens egne innholdsstrømmer (rå eller
Flate-komprimerte), plukker ut de tekstvisende operatorene og dekoder
WinAnsi. Det er et bevisst lite utsnitt av PDF-formatet — nok til
genererte fakturaer, som er de aller fleste.

Sikkerhetsventilen er viktigere enn dekningen: en PDF med egen
fontkoding gir bytes som ikke er tekst, og da returnerer modulen
**None** i stedet for mojibake. Testet med en egen «søppel-PDF».
Bilder hoppes over uten forsøk.

## Heuristikken: kontrollsifrene gjør jobben

`regnmed-core::voucher_hints` leser teksten linje for linje. Det som gjør
den tålelig presis uten modeller er **kontrollsifrene vi allerede har
validatorer for**:

- ni siffer som passerer orgnr-MOD11, nær ordet «orgnr»/«MVA» → orgnr
- sifre som passerer KID-MOD10/MOD11, nær «KID» → KID
- elleve siffer som passerer kontonummer-MOD11, nær «konto» → kontonummer

Tilfeldige tall passerer ikke disse. Testen `tall_uten_gyldig_
kontrollsiffer_foreslas_ikke` holder det ærlig.

Beløp og datoer leses etter nøkkelord, med to regler som var lette å
ta feil av og derfor er testet:

- **«Å betale» vinner** over «sum» og «totalsum» — det er det eneste
  tallet på en faktura som betyr nøyaktig én ting.
- **En linje med tall holder seg til sin egen linje.** Bare en linje
  UTEN tall ser på neste (etikett over verdi er en vanlig layout).
  Uten den regelen ville totalen under «MVA» blitt lest som mva.
- **Mva er aldri mer enn en femtedel av totalen** (25 % av netto er 20 %
  av brutto). Alle linjer som ser ut som mva samles, og den SISTE som
  holder seg innenfor velges — oppsummeringen kommer etter varelinjene,
  så en kolonneoverskrift «Mva» over første varelinje (som «faller
  gjennom» til linjens beløp) taper. Finnes ingen slik linje, foreslås
  ingen mva; det er bedre enn et galt tall.
- **«Org.nr … MVA» er Foretaksregister-merkingen, ikke en mva-linje.**
  En linje med «org» i seg, eller med et gyldig orgnr blant tallene,
  leses aldri som mva. Funnet på regnmeds egen faktura-PDF, der
  «Org.nr 790 292 642 MVA» ble lest som 790 292 642 kr i mva.

**Kvitteringer er ikke fakturaer.** En kasselapp sier «TOTALT» eller
«Bankkort», aldri «å betale», og har sjelden «fakturadato». Derfor tre
svakere regler som bare fyller et tomt felt:

- et beløp etter en linje som STARTER med «totalt», «total», «til
  betaling», «sum», «bankkort» eller «betalt» — og bare med desimaler,
  så «Totalt antall varer 5» ikke blir 5 kroner; aldri på en
  «sum mva»-linje;
- den første datoen i dokumentet, når ingen dato har etikett
  (begrunnelse: «første dato i dokumentet»);
- den øverste linjen som ser ut som et navn — på en kasselapp er det
  butikken.

Hvert funn bærer sin begrunnelse («etter «å betale»», «ni siffer med
gyldig kontrollsiffer nær «orgnr»»), og begrunnelsen vises i UI-et.
Et forslag man ikke kan overprøve raskt er verre enn ingen forslag.

## Kontoforslaget kommer fra din egen historikk

Når orgnr matcher en leverandør i reskontroen, foreslås **kontoen
samme leverandør sist ble bokført på** — en ren spørring over
selskapets egne bilag, ikke en modell og ikke en bransjeantakelse.
Begrunnelsen sier hvilken dato den stammer fra.

Mangler leverandøren, blir det en advarsel («orgnr … finnes ikke i
leverandørreskontroen»), ikke en stille opprettelse.

## Endpoints

- `GET /companies/{id}/inbox/{doc}/forslag` — kilde, felter,
  begrunnelser, kontoforslag, advarsler
- `GET /companies/{id}/expenses/{eid}/suggestion` — det samme for en
  utleggskvittering (docs/utlegg.md)
- `GET /companies/{id}/inbox/{doc}/ehf` — den strukturerte lesingen av
  en EHF, for den som vil se dokumentet slik det er (docs/ehf.md)

## Tester

- `regnmed-core/src/pdftext.rs` — round-trip mot vår egen
  PDF-generator, escapede parenteser, Flate-strøm, «ikke en PDF»,
  bilde-PDF og mojibake (begge gir None).
- `regnmed-core/src/voucher_hints.rs` — en vanlig norsk faktura leses
  komplett; ugyldige kontrollsifre foreslås ikke; verdi under etikett;
  «å betale» slår andre summer; beløpsformater; en kasselapp slik
  `regnmed-ocr` faktisk leste den (støy og alt), og at en
  kvitteringstotal aldri slår et fakturanøkkelord.
- `regnmed-ocr/src/lib.rs` — linjesammensetningen (etikett og beløp på
  én linje, tettrykte linjer holdes fra hverandre); den ekte motoren
  på `tests/receipt.jpg` når `OCR_MODELS_DIR` er satt (ellers hoppes
  den over, som DB-testene).
- `regnmed-api/tests/grupper/bilagstolkning.rs` (ekte Postgres, også CI): en
  faktura-PDF generert av vår egen writer lastes opp som om den kom
  utenfra, og tolkningen finner igjen alle tallene + leverandøren på
  orgnr + kontoen fra historikken; dokumentet forblir ubesluttet; et
  skannet bilde gir `kilde: "ingen"`; EHF går den eksakte veien
  gjennom samme endepunkt.

## Bevisst utenfor

Skyavhengighet i kjernepathen, og enhver automatisk bokføring.
Skannede PDF-er (bilde uten tekstlag inni en PDF) leses ikke ennå —
tekstleseren tar bare JPEG og PNG. Æ, ø og å krever en annen
gjenkjenningsmodell. En LLM med syn (kundens egen, docs/agent.md §8)
som leser kvitteringen hører til agent-fasen: den må lese og fylle ut
gjennom agentens egen identitet og verktøy, ikke gjennom en bakdør i
API-et.
