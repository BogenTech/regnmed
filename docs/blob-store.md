# Dokumenter i objektlagring

Bilagsvedlegg, innboksdokumenter, kvitteringer på utlegg, mottatt e-post
(rå melding og vedlegg) og vedlegg til balansedokumentasjon lagres i en
S3-kompatibel objektlagring, ikke i databasen (migrasjon 0070, 2026-11-08).
Databasen beholder raden — metadata, hvem, når, og dokumentets SHA-256 —
mens selve bytene ligger i lagringen under nøkkelen
`<prefiks><sha256 i hex>`.

## Hvorfor

Dokumentene er det som vokser: bilder og PDF-er er hundrevis av ganger
større enn posteringene. I databasen gjorde de hver nattlige `pg_dump`,
hver ukentlig gjenopprettingsøvelse og hver vacuum tregere i takt med
kundemassen, og de fylte diskene på databaseboksene. Objektlagringen er
laget for nettopp dette.

## Garantien er den samme som før

- **Hashen sjekkes ved hver lesing.** Bytene hentes fra raden hvis den
  fortsatt har dem, ellers fra lagringen, og SHA-256 sammenlignes med
  hashen i raden før noe leveres. Et endret objekt avvises som endrede
  byte i databasen ble.
- **Et manglende objekt er en feil, aldri et tomt dokument.**
  `verify-ledger` og revisjonsrapportens kontroll av vedlegg leser hvert
  dokument og feiler høylytt hvis et objekt er borte.
- **Innholdsadressert.** Samme byte er ett objekt uansett hvor mange rader
  som peker på det: et innboksdokument som bokføres blir et bilagsvedlegg
  uten at noe kopieres, og en ny opplasting av samme fil skriver over
  seg selv med identiske byte.
- **Objektet skrives før raden.** En rad peker aldri på et objekt som ikke
  finnes; feiler transaksjonen etterpå, blir det bare liggende et objekt
  ingen rad peker på.

## Flytting av eksisterende dokumenter

`regnmed offload-documents` går gjennom alle seks kolonnene: skriver
bytene til lagringen, leser dem tilbake og sammenligner, og tømmer først
da radens innholdskolonne. Kommandoen kan kjøres på nytt og avbrytes;
en rad er enten urørt eller beviselig i lagringen.

Tabellene er fortsatt bare-innsetting. Migrasjon 0070 gir vaktene ett
unntak, samme form som faktureringsmerket på timer: innholdskolonnen kan
gå fra byte til NULL, forutsatt at alle andre kolonner er uendret og at
den lagrede hashen ER hashen av bytene som fjernes (for rå e-post, som
manglet hash, settes den i samme oppdatering). Innholdet kan aldri komme
tilbake eller byttes ut. Testen
`an_attachment_row_allows_only_a_verified_offload` (regnmed-db) fester det.

## Konfigurasjon

Seks variabler, alle eller ingen (`regnmed-db::blob`):
`BLOB_S3_ENDPOINT`, `BLOB_S3_BUCKET`, `BLOB_S3_REGION`, `BLOB_S3_PREFIX`,
`BLOB_S3_ACCESS_KEY`, `BLOB_S3_SECRET_KEY`. Uten dem lagres dokumentene i
databasen som før (lokal utvikling, CI). En halv konfigurasjon nektes ved
oppstart. I clusterne kommer de fra Secret-en `blob-store`
(valgfri `envFrom` på alle regnmed-containere, docs/deploy.md).

| Miljø | Bøtte | Prefiks |
|---|---|---|
| prod (Hetzner) | `bogenhuset` (hel1) | `regnmed-prod/documents/` |
| test (homelab) | `bogenhuset` (hel1) | `regnmed-test/documents/` |

S3-klienten er håndskrevet (PUT og GET med AWS Signature V4 over sha2 og
reqwest), testet mot AWS' publiserte eksempelsignatur og mot Hetzner.

## Ikke gjort ennå

- **Versjonering eller object lock på bøtta.** Hashen avslører et endret
  eller slettet objekt, men gjenoppretter det ikke. Til det trengs
  versjonering (eller WORM) på bøtta — et oppfølgingssteg.
- **`payment_run.file`** (noen kB pain.001-XML på en kjøringsrad som
  endres gjennom statusene) ligger fortsatt i databasen.
