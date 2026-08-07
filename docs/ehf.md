# EHF: elektronisk faktura ut og inn

Issue #14. EHF er UBL 2.1 med PEPPOL-profil (BIS Billing 3.0):
**obligatorisk mot det offentlige**, forventet i B2B. Denne saken
leverer de to endene vi kan stå inne for uten et aksesspunkt:
dokumentet vi **sender**, og dokumentet vi **leser**.

## To tiere, som for bank

| Tier | Status |
| --- | --- |
| **Dokumentet** — rendre gyldig EHF ut, lese mottatt EHF inn | ✅ her |
| **Transporten** — sending/mottak gjennom et aksesspunkt (Peppol SMP-oppslag, AS4) | krever avtale med en aksesspunktleverandør |

Egen AP-sertifisering er bevisst utenfor (ROADMAP: frugal). Når
transporttieren kommer, er det **sendingen** som logges — samme mønster
som e-postutsendelsen (docs/faktura.md): loggraden er meldingen.

## Ut: rendret fra fakturaens egne rader

`GET /companies/{id}/invoices/{iid}/ehf` bygger dokumentet fra den
utstedte fakturaens låste rader og rendrer det med
`regnmed-core::ehf` — hand-rolled og deterministisk, som SAF-T,
mva-meldingen og pain.001. Samme faktura gir byte-identisk XML for
alltid.

Merk hva som **ikke** skjer: EHF-en lagres ikke som vedlegg slik
faktura-PDF-en gjør. PDF-en *er* salgsdokumentet
(bokføringsforskriften §5-1, oppbevaringsplikt fra utstedelsen);
EHF-en er en transportkonvolutt utledet av de samme uforanderlige
tallene. Å lagre begge ville vært to sannheter om samme faktura.

Detaljer verdt å vite:

- Deltakeridentifikator er norsk orgnr under ISO 6523 **ICD 0192** —
  både `EndpointID`, `PartyIdentification` og `PartyLegalEntity/CompanyID`.
- Mva-satsen på hver linje er den som gjaldt **på fakturadatoen**
  (samme daterte oppslag som posteringen brukte), så dokumentet kan
  ikke drive fra hovedboken.
- Ett `TaxSubtotal` per sats; en linje uten mva-kode blir nullsats (Z),
  ikke utelatt.
- Kreditnota bruker `CreditNote`-elementene, koden 381, og navngir
  fakturaen den krediterer i `BillingReference`. Ingen forfallsdato,
  ingen betalingsinformasjon.
- **Leveringstidspunktet** (`cac:Delivery/cbc:ActualDeliveryDate`,
  BT-72) følger med på både faktura og kreditnota — lovpålagt på
  salgsdokumentet uansett kanal (bokføringsforskriften §5-1-1 nr. 4,
  docs/faktura.md). `cac:Delivery` ligger mellom kjøperparten og
  betalingsopplysningene; UBL-sekvensen er bundet, og XSD-kjøringen i
  testene og CI fanger feil plassering. Fakturaer utstedt før #81 har
  ingen registrert leveringsdato, og da utelates elementet helt
  framfor å gjette.
- Adressen vår er fritekst i masterdata og EHF vil ha den delt; en form
  vi kjenner igjen («Storgata 1, 0155 Oslo») deles, en vi ikke kjenner
  igjen går ut hel som gatelinje. Bedre en komplett adresse i ett felt
  enn en gal fordeling.
- Mangler mottakeren organisasjonsnummer, sier endepunktet fra i stedet
  for å sende en tom id — i Peppol *er* orgnr adressen.

### Ærlig begrensning

XSD-validering (offisiell UBL 2.1, vendored i `docs/ehf/`, kjørt med
xmllint i tester og CI) beviser at dokumentet er **velformet UBL** —
ikke at det oppfyller alle PEPPOL BIS-forretningsreglene. De er
Schematron, og kjøres av aksesspunktet ved innsending. Det vi vet er
verifisert: profil-id-ene, deltakeridentifikatorene, at beløpene
summerer, og at strukturen er skjemagyldig.

Kategoriene E (fritak) og AE (omvendt avgiftsplikt) utledes ikke
automatisk — de krever en begrunnelsestekst, og å gjette den ville
vært å påstå noe om avgiftsplikt vi ikke vet.

## Inn: originalen lagres, forslaget regnes ut

En mottatt EHF lastes opp i **bilagsinnboksen** som ethvert annet
bilag: innholdet er uforanderlig fra ankomst og hash-sjekket
(docs/bilagsinnboks.md).

`GET /companies/{id}/inbox/{doc}/ehf` leser den lagrede originalen og
returnerer et **bokføringsforslag**: selger (matchet mot
leverandørreskontroen på orgnr), fakturanummer, datoer, KID,
kontonummer, netto/mva/brutto og linjene med sats.

Ingenting utledet lagres. Det gir to ting gratis: originalen er den
eneste sannheten, og et forbedret forslag gjelder også for dokumenter
som allerede ligger i innboksen.

Dette er den strukturerte enden av bilagstolkning (#34): når
dokumentet er EHF, er leverandør og beløp ikke gjetning — de står i
filen. Mennesket bokfører (eller avviser) fortsatt gjennom innboksen,
med attestering når policyen er på (docs/attestering.md).

Parseren er tolerant i camt.053-stil: den leser bare det bokføringen
trenger, hopper over ukjente elementer, godtar både `Invoice` og
`CreditNote` og hvilket som helst prefiks — vi kontrollerer ikke
avsenderen. Advarsler (ukjent leverandør, netto + mva som ikke
stemmer med forfalt beløp, fremmed valuta) følger forslaget i stedet
for å stoppe det.

## Endpoints

- `GET /companies/{id}/invoices/{iid}/ehf` — EHF for en utstedt faktura/kreditnota
- `GET /companies/{id}/inbox/{doc}/ehf` — bokføringsforslag fra en mottatt EHF

Portal: **EHF**-knapp på fakturalinjene (Faktura), og en **EHF**-knapp
på XML-dokumenter i innboksen som leser filen og fyller ut
bokføringsskjemaet.

## Tester

- `regnmed-core/src/ehf.rs` — profil-id-er, ICD 0192, beløp som
  heltall øre med punktum og valutakode, én avgiftsgruppe per sats,
  kreditnotaens egne elementnavn, escaping, determinisme, og
  **XSD-validering av begge dokumenttyper**.
- `regnmed-core/src/ehf_import.rs` — round-trip mot vår egen renderer
  (det vi skriver, kan vi lese), kreditnota gjenkjent, et fremmed
  dokument med ukjente elementer og uten linjesats, avvisning av noe
  som ikke er EHF, beløpsformater.
- `regnmed-api/tests/grupper/ehf.rs` (ekte Postgres, også CI): faktura fra
  databasen rendret og **XSD-validert**, adressen delt riktig, KID med,
  kreditnota som peker tilbake; mottatt EHF i innboksen gir forslag med
  leverandøren matchet på orgnr, originalen står urørt, og et dokument
  som ikke er EHF sier fra.
