# E-post-inn til bilagsinnboksen

Issue #35. Leverandører og ansatte sender kvitteringer på e-post. De
skal havne i bilagsinnboksen uten at noen logger inn i portalen — og
uten at innboksen blir et sted hvem som helst kan dumpe hva som helst.

## Én mail-rail i plattformen

Utgående post går ut på `regnid.mail.send` (docs/faktura.md, #32).
Innkommende post kommer inn på **`regnid.mail.received`**, publisert av
den samme infrastrukturen. regnmed reiser ingen egen SMTP-stakk: MX-en
og mottaket bor i regnid (søsterrepo, aldri vendored inn her), og
feltene i `regnmed-api::mailq_in` er wire-kontrakten — samme mønster
som for utgående.

Uten `NATS_URL` finnes ingen konsument, og e-post-inn er ganske enkelt
av. Portalen sier det, i stedet for å vise en adresse som ikke tar imot
noe.

Konfigurasjon: `NATS_URL` (railen) og `MAIL_IN_DOMAIN` (domenet
adressene hører til, f.eks. `mottak.regnmed.no`).

## Adressen er en kapabilitet

Hvert selskap får `bilag-<navn>-<tilfeldig>@<domene>`. Navnedelen er
der for menneskene — adressen skal kunne leses opp over telefon.
Halen er tilfeldig fordi den som kjenner adressen kan **levere** noe i
innboksen; den må ikke kunne gjettes ut fra firmanavnet.

Adressen gir bare rett til å levere. Aldri til å lese, aldri til å
bestemme noe. Lekker den, roterer man den: den nye finnes, den gamle
slutter å ta imot i samme øyeblikk (`company_mail_inbox`, append-only —
en tilbakekalt adresse kan ikke gjenoppstå).

## Ukjent avsender ⇒ karantene

Avsenderlisten er admin-styrt og tar enten en full adresse
(`post@grossisten.no`) eller et helt domene (`@grossisten.no`).
Ingenting annet: et jokertegn ingen klarer å lese er et sikkerhetshull
med vennlig ansikt.

| Situasjon | Utfall |
| --- | --- |
| Avsender på listen, e-posten har vedlegg | Vedleggene blir innboksdokumenter med én gang |
| Avsender ikke på listen | **Karantene** — lagret helt, ingen dokumenter, venter på admin |
| Ingen vedlegg | Avvist, men logget med begrunnelse |
| Samme Message-Id igjen | Ingen dublett (køer gjentar seg) |

De to alternativene til karantene er begge verre: stille import lar hvem
som helst fylle innboksen, og stille forkasting får et bilag noen
faktisk sendte til å forsvinne. En admin **slipper inn** (valgfritt med
«legg avsenderen til på listen») eller **avviser med begrunnelse** — og
raden blir stående uansett.

## Hva som lagres

- `inbox_mail` — insert-only logg: avsender, emne, **brødteksten**
  (dokumentasjon av opprinnelse), antall vedlegg, status og
  begrunnelse. Uforanderlig; bare karantene kan avgjøres, og bare én
  gang (trigger).
- `inbox_mail_attachment` — vedleggene dekodet én gang ved mottak, med
  SHA-256. Derfor kan en karantene slippes gjennom senere uten at
  avsenderen sender på nytt, og uten at vi tolker den lagrede meldingen
  om igjen.
- `inbox_document.inbox_mail_id` — opprinnelsen til et dokument som kom
  med e-post. Kolonnen ligger utenfor 0015-grantet, og vakten der er
  utvidet til å nevne den: grants OG trigger, som doktrinen krever.

Dokumentene lages gjennom nøyaktig samme uforanderlige vei som en
opplasting: innhold hash-sjekket ved ankomst, ingen beslutning tatt.
`uploaded_by` er **avsenderadressen** — det er den som rakte oss
bilaget, og innboksen viser det.

E-post bokfører ingenting. Bilagstolkning (docs/bilagstolkning.md) og
attestering (docs/attestering.md) gjelder som for ethvert annet
dokument.

## Endpoints

- `GET    /companies/{id}/inbox/settings` — adresse + avsenderliste
- `POST   /companies/{id}/inbox/settings/address` — ny adresse, roterer (admin)
- `POST   /companies/{id}/inbox/settings/senders` — `{sender, note}` (admin)
- `DELETE /companies/{id}/inbox/settings/senders/{sid}` (admin)
- `GET    /companies/{id}/inbox/mail[?status=karantene]` — mottaksloggen
- `POST   /companies/{id}/inbox/mail/{mid}/release` — `{tillat_avsender}` (admin)
- `POST   /companies/{id}/inbox/mail/{mid}/reject` — `{note}` (admin)

Loggen er lesbar for alle med tilgang: en revisor skal kunne se hva som
kom inn og hva som ble gjort med det.

Portal: **E-post-inn**-kort i Bilag — adressen, karantenekøen med
Slipp inn / Avvis, avsenderlisten og de siste mottakene.

## Tester

- `regnmed-core/src/epost.rs` — adressen er lesbar men ikke gjettbar og
  holder skjemaets lengdekrav; avsender leses ut av visningsnavn;
  listen godtar adresse og domene, og et domene må matche helt
  (`grossisten.no.svindel.no` slipper ikke inn).
- `regnmed-api/tests/epost_inn.rs` (ekte Postgres + ekte `nats-server`,
  hoppes over uten): fire meldinger på railen gir dokument, karantene,
  avvist-uten-vedlegg og ingen dublett; brødteksten er lagret;
  `uploaded_by` er avsenderadressen; karantene lager ingen dokumenter
  før admin slipper den inn; en avgjort e-post kan ikke avgjøres om
  igjen; og en rotert adresse tar ikke imot noe.

## Bevisst utenfor

Egen SMTP-mottakelse i regnmed (railen er felles), automatisk
bokføring av mottatte bilag (finnes ikke noe sted i systemet), og
størrelsesgrenser utover 20 MB per vedlegg — større filer avvises med
begrunnelse i loggen.
