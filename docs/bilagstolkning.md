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

`regnmed-core::pdftekst` leser PDF-ens egne innholdsstrømmer (rå eller
Flate-komprimerte), plukker ut de tekstvisende operatorene og dekoder
WinAnsi. Det er et bevisst lite utsnitt av PDF-formatet — nok til
genererte fakturaer, som er de aller fleste.

Sikkerhetsventilen er viktigere enn dekningen: en PDF med egen
fontkoding gir bytes som ikke er tekst, og da returnerer modulen
**None** i stedet for mojibake. Testet med en egen «søppel-PDF».
Bilder hoppes over uten forsøk.

## Heuristikken: kontrollsifrene gjør jobben

`regnmed-core::bilagstolk` leser teksten linje for linje. Det som gjør
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

- `regnmed-core/src/pdftekst.rs` — round-trip mot vår egen
  PDF-generator, escapede parenteser, Flate-strøm, «ikke en PDF»,
  bilde-PDF og mojibake (begge gir None).
- `regnmed-core/src/bilagstolk.rs` — en vanlig norsk faktura leses
  komplett; ugyldige kontrollsifre foreslås ikke; verdi under etikett;
  «å betale» slår andre summer; beløpsformater.
- `regnmed-api/tests/bilagstolkning.rs` (ekte Postgres, også CI): en
  faktura-PDF generert av vår egen writer lastes opp som om den kom
  utenfra, og tolkningen finner igjen alle tallene + leverandøren på
  orgnr + kontoen fra historikken; dokumentet forblir ubesluttet; et
  skannet bilde gir `kilde: "ingen"`; EHF går den eksakte veien
  gjennom samme endepunkt.

## Bevisst utenfor

Skyavhengighet i kjernepathen, og enhver automatisk bokføring.
OCR for skannede bilder er neste steg som sidecar — samme endepunkt,
samme svarform, bare en kilde til.
