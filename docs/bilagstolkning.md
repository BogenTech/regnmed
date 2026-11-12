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
| `ingen` | Skannet bilde uten tekstlag | Ingenting foreslås, og det sies fra |

En skannet faktura gir altså **ingen** felter — ikke gjetninger. OCR
hører til en valgfri sidecar priset mot ressursbudsjettet
(docs/frugality.md); API-et oppfører seg identisk uten den.

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
- `GET /companies/{id}/inbox/{doc}/ehf` — den strukturerte lesingen av
  en EHF, for den som vil se dokumentet slik det er (docs/ehf.md)

## Tester

- `regnmed-core/src/pdftext.rs` — round-trip mot vår egen
  PDF-generator, escapede parenteser, Flate-strøm, «ikke en PDF»,
  bilde-PDF og mojibake (begge gir None).
- `regnmed-core/src/voucher_hints.rs` — en vanlig norsk faktura leses
  komplett; ugyldige kontrollsifre foreslås ikke; verdi under etikett;
  «å betale» slår andre summer; beløpsformater.
- `regnmed-api/tests/grupper/bilagstolkning.rs` (ekte Postgres, også CI): en
  faktura-PDF generert av vår egen writer lastes opp som om den kom
  utenfra, og tolkningen finner igjen alle tallene + leverandøren på
  orgnr + kontoen fra historikken; dokumentet forblir ubesluttet; et
  skannet bilde gir `kilde: "ingen"`; EHF går den eksakte veien
  gjennom samme endepunkt.

## Bevisst utenfor

Skyavhengighet i kjernepathen, og enhver automatisk bokføring.
OCR for skannede bilder er neste steg som sidecar — samme endepunkt,
samme svarform, bare en kilde til.
