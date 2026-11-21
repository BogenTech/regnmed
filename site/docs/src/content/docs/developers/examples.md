---
title: Eksempler
description: Ferdige curl-oppskrifter for de vanligste flytene, bygget fra de faktiske forespørselsstrukturene og testene.
sidebar:
  order: 6
---

Alle eksemplene forutsetter

```sh
API=https://api.regnmed.no
COMPANY=<selskapets id fra /me>
TOKEN=<access_token, se «Kom i gang»>
AUTH="Authorization: Bearer $TOKEN"
```

Kroner skrives som øre: 1 250,00 kr er `125000`. Datoer er `YYYY-MM-DD`. Kravene til rolle står i [API-referansen](/developers/api/).

## 1. Hvilke selskaper kan jeg arbeide for?

```sh
curl -s $API/me -H "$AUTH" | jq '.companies[] | {company_id, name, access, rettigheter}'
```

Kall dette først i en økt. `rettigheter` forteller hva du har lov til, før du prøver noe som blir nektet.

## 2. Saldobalanse for en periode

```sh
curl -s "$API/companies/$COMPANY/reports/saldobalanse?from=2026-01-01&to=2026-12-31" -H "$AUTH"
```

```json
{ "accounts": [
  { "number": "1920", "name": "Bankinnskudd",
    "inngaende_ore": 0, "debet_ore": 12500000, "kredit_ore": 4200000, "utgaende_ore": 8300000 }
] }
```

Resultat og balanse har egne endepunkter: `…/reports/resultat?from=&to=` og `…/reports/balanse?date=`. Kontospesifikasjon for én konto: `…/reports/kontospesifikasjon?from=&to=&account=1920`.

## 3. Bokfør et manuelt bilag

Summen av linjene må være null (debet positiv, kredit negativ). Kravet til rolle er `BILAG_BOKFOR`.

```sh
curl -s -X POST $API/companies/$COMPANY/vouchers -H "$AUTH" -H 'Content-Type: application/json' -d '{
  "journal_code": "GL",
  "date": "2026-03-01",
  "description": "Overføring til skattetrekk",
  "lines": [
    { "account": "1921", "amount_ore": 500000 },
    { "account": "1920", "amount_ore": -500000 }
  ]
}'
```

```json
{ "voucher_id": "0192f…", "voucher": "2026-14", "chain_seq": 14 }
```

En linje kan også ha `vat_code`, `party_no`, `description`, `avdeling` og `prosjekt`. Bilaget avvises med `400` og en forklarende melding ved ubalanse, ukjent eller deaktivert konto, låst periode, eller mva-kode som selskapet ikke kan bruke. Et bilag rettes aldri. Du reverserer det:

```sh
curl -s -X POST $API/companies/$COMPANY/vouchers/$VOUCHER_ID/reverse -H "$AUTH" \
  -H 'Content-Type: application/json' -d '{ "dato": "2026-03-02" }'
```

## 4. Opprett en kunde og en faktura

```sh
curl -s -X POST $API/companies/$COMPANY/parties -H "$AUTH" -H 'Content-Type: application/json' -d '{
  "kind": "kunde", "name": "Kunde AS",
  "email": "regnskap@kunde.example", "address": "Storgata 1, 0155 Oslo"
}'
# { "party_id": "…", "party_no": "10001" }

curl -s -X POST $API/companies/$COMPANY/invoices -H "$AUTH" -H 'Content-Type: application/json' -d '{
  "party_no": "10001",
  "invoice_date": "2026-02-01",
  "due_date": "2026-02-15",
  "lines": [{
    "description": "Konsulentbistand",
    "quantity_milli": 2500,
    "unit_price_ore": 400000,
    "vat_code": "3"
  }]
}'
```

```json
{
  "invoice_id": "…", "invoice_no": 1001, "kid": "…",
  "net_ore": 1000000, "vat_ore": 250000, "gross_ore": 1250000,
  "gross_nok_ore": 1250000, "voucher": "2026-15"
}
```

Fakturanummer og KID tildeles atomisk med bilaget. Mva-satsen slås opp fra **fakturadatoen**. `vat_code` `3` er utgående 25 %. Standardverdier: journal `GL`, kundefordring `1500`, mva-konto `2700`, inntektskonto `3000` (eller produktets). Valgfrie felt: `leveringsdato`, `leveringssted`, `valuta`, `produkt` per linje, `timer_entry_ids` for å fakturere ufakturerte timer og `buyer_reference`.

Fakturaen er **uforanderlig**. Feil rettes med kreditnota:

```sh
curl -s -X POST $API/companies/$COMPANY/invoices/$INVOICE_ID/credit-note -H "$AUTH"
```

PDF og EHF hentes med `GET …/invoices/{id}/pdf` og `GET …/invoices/{id}/ehf`.

## 5. Send fakturaen

```sh
curl -s -X POST $API/companies/$COMPANY/invoices/$INVOICE_ID/send -H "$AUTH" \
  -H 'Content-Type: application/json' -d '{ "email": "annen@kunde.example" }'
```

```json
{ "job_id": "…", "status": "queued" }
```

Svaret er `202`: en arbeider velger EHF eller e-post og sender. Hent utfallet med `GET $API/companies/$COMPANY/jobs/$JOB_ID`. Når `status` er `done`, sier `result` hvilken `kanal` som ble brukt.

Kroppen er valgfri. Uten `email` brukes kundens lagrede adresse. Kravet er `FAKTURA_SEND`. Uten konfigurert e-postskinne (`NATS_URL`) får du `400` med en tydelig melding i stedet for at utsendelsen late som om den gikk.

## 6. Last opp et bilag til innboksen

Filen sendes som rå kropp, med filnavnet og SHA-256 i spørringen:

```sh
FILE=kvittering.pdf
SHA=$(shasum -a 256 "$FILE" | cut -d' ' -f1)

curl -s -X POST "$API/companies/$COMPANY/inbox?filename=$FILE&sha256=$SHA" \
  -H "$AUTH" -H 'Content-Type: application/pdf' --data-binary @"$FILE"
# { "document_id": "…", "existing": false }
```

Kjører du det samme kallet på nytt, får du `existing: true` og ingen duplikat. Kravet er `BILAG_LAST_OPP`.

Slik fortsetter flyten. Forslaget er **et forslag og aldri en bokføring**:

```sh
# regnmeds eget forslag, med begrunnelse per felt
curl -s $API/companies/$COMPANY/inbox/$DOC_ID/forslag -H "$AUTH"

# bokfør dokumentet (summen må være null)
curl -s -X POST $API/companies/$COMPANY/inbox/$DOC_ID/bokfor -H "$AUTH" \
  -H 'Content-Type: application/json' -d '{
  "journal_code": "GL", "date": "2026-03-05", "description": "Kontorrekvisita",
  "lines": [
    { "account": "6800", "amount_ore": 80000, "vat_code": "1" },
    { "account": "2710", "amount_ore": 20000 },
    { "account": "2400", "amount_ore": -100000, "party_no": "20001" }
  ],
  "due_date": "2026-03-20", "kid": "1234567"
}'

# eller avvis, med begrunnelse som blir stående
curl -s -X POST $API/companies/$COMPANY/inbox/$DOC_ID/avvis -H "$AUTH" \
  -H 'Content-Type: application/json' -d '{ "note": "Hører ikke hjemme i dette regnskapet" }'
```

Ved en aktiv attesteringspolicy må et menneske godkjenne før `bokfor` lykkes, og samme person kan ikke både attestere og bokføre.

## 7. Eksporter SAF-T

```sh
curl -s -OJ "$API/companies/$COMPANY/reports/saft?year=2026" -H "$AUTH"
# eller et vilkårlig intervall: ?from=2026-01-01&to=2026-06-30
```

Svaret er en XML-fil (`SAF-T Financial_<orgnr>_<tidspunkt>.xml`), validert mot Skatteetatens XSD. Kontaktperson i headeren tas fra tokenets navn. For en maskin uten fullt navn, send `contact_first=` og `contact_last=`.

## 8. Mva-spesifikasjon for en termin

```sh
curl -s "$API/companies/$COMPANY/reports/mva?year=2026&termin=2" -H "$AUTH"
```

```json
{
  "year": 2026, "termin": 2, "ordning": "to-maneder", "frist": "2026-06-10",
  "lines": [{ "code": "3", "description": "…", "rate_bp": 2500,
              "grunnlag_ore": 1000000, "avgift_ore": 250000, "virkning_ore": 250000,
              "retning": "utgaende" }],
  "utgaende_ore": 250000, "inngaende_ore": 0, "netto_ore": 250000
}
```

Terminnummeret følger selskapets terminordning (`GET …/mva/terminordning`). Et nummer utenfor ordningen gir `400`. Postene bak én linje: `…/reports/mva/postings?year=&termin=&kode=3`. Selve mva-meldingen: `…/reports/mva-melding`.

## 9. Åpne poster og kundereskontro

```sh
# kunder og deres saldo
curl -s "$API/companies/$COMPANY/parties?kind=kunde&sort=overdue&desc=true&limit=10" -H "$AUTH"

# åpne poster for én kunde
curl -s "$API/companies/$COMPANY/parties/$PARTY_ID/items?open=true" -H "$AUTH"
```

```json
{ "items": [{ "entry_id": "…", "voucher": "2026-15", "date": "2026-02-01",
              "due_date": "2026-02-15", "kid": "…",
              "amount_ore": 1250000, "matched_ore": 0, "remaining_ore": 1250000 }] }
```

Forfalte fakturaer med alder og foreslått neste purresteg: `GET …/invoices/overdue`. Samlet kunde- og leverandørspesifikasjon: `…/reports/kundespesifikasjon` og `…/reports/leverandorspesifikasjon`.

## 10. Verifiser hovedboken

```sh
curl -s $API/companies/$COMPANY/anchors/verify -H "$AUTH"
# { "ok": true, "vouchers_checked": 1523, "attachments_checked": 311, "anchors_checked": 42, "problems": [] }
```

Se [Verifiser selv](/developers/verify-it-yourself/). Revisjonsrapporten med åtte kontroller: `GET …/reports/revisjon` (legg til `?format=tekst` for en nedlastbar tekstversjon).
