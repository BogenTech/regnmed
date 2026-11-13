# E-faktura ut fra 1. januar 2027 — forslag

**Status: FORSLAG, ikke bygget** (2026-11-13). Oppfølger til #14
(dokumenttieren, docs/ehf.md) og #69 (transporten). Dokumentet sier hva
loven krever, hvilke veier som finnes, hvordan det passer inn i
regnmed, og hva som må gjøres utenfor koden. Avgjørelsene som står åpne
er samlet i §6.

## 1. Hva loven krever

### 1.1 Kildene

| Kilde | Hva | Verifisert |
| --- | --- | --- |
| LOV-2026-06-19-39 | endrer bokføringsloven §§ 3, 7, 10, 11, 13 | endringsteksten lest på Lovdata (LTI) |
| Res. 19. juni 2026 nr. 1154 | ikrafttredelse: §§ 3, 10, 11, 13 fra 1.1.2027, § 7 fra 1.1.2030 | via Lovdatas endringsnoter, ikke resolusjonen selv |
| FOR-2026-09-25-1913 | utsetter **mottaksplikten** til 1.1.2030 | lest på Lovdata (LTI) |
| FOR-2026-09-29-1933 | endrer bokføringsforskriften §§ 1-1, 1-2, 5-1-2, 5-2-1, 5-2-9, 7-7, 7-8, 8-13-2, 8-14-2 | lest på Lovdata (LTI), gjengitt i sammendrag |
| Skatteetaten, «Plikt til å sende e-faktura fra 1. januar 2027» + uttalelse 30.9.2026 | etatens tolkning, bl.a. ELMA-regelen | lest 2026-11-13 |

### 1.2 Plikten

**Bokføringsloven § 3 nr. 3** (ny): elektronisk faktura er «et
salgsdokument som kan utstedes, sendes og mottas i et strukturert,
elektronisk format, som er egnet for automatisert behandling i
regnskapssystemet». En PDF på e-post er altså **ikke** e-faktura.

**Bokføringsloven § 10 annet ledd** (ny, i kraft 1.1.2027):
«Dokumentasjon for salg av varer og tjenester til andre
bokføringspliktige skal utstedes i elektronisk fakturaformat, jf. § 3
nr. 3.» Annet punktum gir kjøperen tilsvarende plikt til å **motta** —
utsatt til **1.1.2030** av FOR-2026-09-25-1913.

**Hvem det gjelder fram til 2030 — ELMA-regelen.** Lovteksten selv
betinger ikke plikten av at kjøperen kan motta, og nevner ikke ELMA.
Det gjør Skatteetaten: fordi mottaksplikten først gjelder fra 2030, vil
utstedelsesplikten «i praksis bare gjelde når den bokføringspliktige
mottakeren har funksjonalitet for å motta slike fakturaer», og
«Du kan derfor bare bruke PDF-faktura når kjøperen ikke er registrert i
ELMA-registeret» (eller tilsvarende Peppol-register/SMP). Dette er
**etatens tolkning, ikke forskriftstekst** — men det er den regelen
kontrollen vil bruke, og den er den regnmed må bygge etter.

Praktisk regel for regnmed fra 1.1.2027:

> Er kjøperen registrert som mottaker av dokumenttypen i Peppol-nettet
> (ELMA eller en annen SMP), skal fakturaen/kreditnotaen sendes som EHF.
> Ellers er PDF tillatt.

Salg til forbrukere og kontantsalg er utenfor (Skatteetaten).

**Formatet — forskriften § 5-2-9 nytt annet ledd** (i kraft 1.1.2027):
EHF Fakturering, Peppol BIS Billing, EHF Selvfakturering og Peppol BIS
Self-Billing, **versjon 3.0 eller nyere**. Det vi rendrer i dag
(`regnmed-core::ehf`, BIS Billing 3.0) er godkjent format.

**Overgangsregel 1.1.2027–31.12.2029** (endringsforskriften): partene
kan **avtale** et annet e-fakturaformat (Skatteetaten nevner EDIFACT og
E2B, og at nettbaserte fakturaløsninger kan brukes ut 2029). Ingen av
dem er aktuelle for regnmed; vi trenger bare BIS 3.0.

### 1.3 Unntaket for lav omsetning — forskriften § 1-2

Ny § 1-2 (i kraft 1.1.2027): bokføringspliktige som leverer
skattemelding for formues- og inntektsskatt og har **omsetning på
høyst 50 000 kr per kalenderår**, er unntatt fra bokføringsloven
§§ 5–9 og § 13b (men ikke dokumentasjonsreglene). Plikten til å
**utstede** e-faktura gjelder for dem **bare hvis faktureringssystemet
støtter det**, og fra 1.1.2030 er de unntatt fra **mottaksplikten**.
Unntaket faller bort fra det tidspunktet terskelen overskrides, og
gjelder ikke regnskapspliktige, mva-registrerte (skatteforvaltningsloven
§ 8-3) med flere.

Konsekvens for regnmed: når regnmed støtter EHF-utsendelse **for
kunden**, gjelder utstedelsesplikten også for disse kundene — unntaket
«hvis systemet støtter det» slår ikke inn. Det forutsetter at EHF er
inkludert for alle; blir EHF et betalt tillegg eller får et tak
(beslutning 2 i §6), støtter systemet det *ikke* for en kunde uten
tillegget, og da gjelder unntaket for kunder under terskelen. Med EHF
inkludert trengs **ikke noe flagg før 2030**, og da bare for
mottaksplikten. (Skill-notatet foreslo et flagg; for utsendelse ville
det vært et flagg ingen ting leser — så lenge beslutning 2 er «inkludert».)

### 1.4 Det som kommer etter

| Dato | Regel | Konsekvens i regnmed |
| --- | --- | --- |
| 1.1.2027 | § 13 annet ledd (ny): «Elektronisk faktura, jf. § 3 nr. 3, skal likevel oppbevares i sitt opprinnelige format» | mottatt EHF i innboksen lagres allerede uendret (docs/bilagsinnboks.md) ✅; den **sendte** XML-en lagres — se §3.5 |
| 1.1.2027 | § 7-7 tredje ledd (ny): ved avvikling skal bokførte opplysninger være elektronisk tilgjengelige i minst seks måneder etter opphøret | en kunde som avvikler, må kunne beholde tilgang eller få full eksport (SAF-T + bilag) — gjelder utover denne saken |
| 1.1.2028 | § 5-1-2: kjøperens **orgnr skal alltid** stå på salgsdokumentet ved salg til bokføringspliktig | utstedelse uten orgnr til en virksomhet må nektes eller advares |
| 1.1.2028 | § 5-2-1 fjerde ledd: salgsdokumentet **skal sendes** til bokføringspliktige kjøpere | «usendt faktura» blir et avvik; se §5 om abonnementsfakturaene |
| 1.1.2030 | § 10 annet ledd annet punktum: **motta** e-faktura; § 7 fjerde ledd: bokføring i elektronisk regnskapssystem | hvert selskap registreres som mottaker i ELMA og innkommende EHF går til bilagsinnboksen |

### 1.5 Det jeg ikke har kunnet verifisere

- **Resolusjonen 19. juni 2026 nr. 1154** er lest bare gjennom Lovdatas
  endringsnoter, ikke i fulltekst.
- **Forskriftsendringene** er lest i Lovdatas LTI-versjon, men verktøyet
  gjengir dem i sammendrag; ordlyden i § 1-2 og overgangsregelen bør
  leses ordrett før koden siterer dem.
- **ELMA-regelen** finnes bare i Skatteetatens veiledning og uttalelse,
  ikke i lov eller forskrift. Hvis Finansdepartementet eller
  Skattedirektoratet senere gjør den til forskriftstekst, kan ordlyden
  avvike.
- **Om selgerens egen kopi** av en utstedt e-faktura må oppbevares som
  XML etter § 13 annet ledd tredje punktum, er ikke avklart. Teksten
  sier «elektronisk faktura» uten å skille mellom sendt og mottatt.
  Forslaget lagrer den uansett (§3.5) — det er billig og kan ikke være
  feil.
- **Unntaket i § 7-7 for omsetning under 5 mill.** skal ifølge
  sekundærkilder falle bort fra 1.1.2027; endringsforskriftens § 7-7
  tredje ledd handler derimot om avvikling, og jeg har ikke sett teksten
  som fjerner unntaket.
- **Kreditnota til en mottaker som er registrert for faktura, men ikke
  for kreditnota** i SMP: ingen kilde sier noe uttrykkelig. Forslaget
  sender da PDF og logger hvorfor.

## 2. Transporten: eget aksesspunkt eller leverandør

Peppol er et fire-hjørners nettverk: selgerens system (C1) → selgerens
aksesspunkt (C2) → kjøperens aksesspunkt (C3) → kjøperens system (C4).
Mottakeren slås opp via SML (DNS) → SMP, som sier hvilket aksesspunkt og
hvilke dokumenttyper deltakeren `0192:<orgnr>` tar imot. **ELMA** er den
norske SMP-en (forvaltet av Digdir ifølge anskaffelser.no). Norsk
Peppol-myndighet er **DFØ**.

### 2.1 Eget aksesspunkt

Krever, så vidt jeg har kunnet bringe på det rene:

- OpenPeppol-medlemskap og tjenesteleverandøravtale med Peppol-
  myndigheten (DFØ), inkludert de norske myndighetsspesifikke kravene.
- Peppol PKI-sertifikat, bestått testprosess, og deretter drift av AS4
  (ebMS3 med WS-Security-signering og -kryptering) med høy
  tilgjengelighet — avsendere prøver ikke i det uendelige.
- Kjenn-din-kunde for hver sluttbruker, og OpenPeppols månedlige
  rapportering (TSR/EUSR) — **ikke verifisert i detalj**, men del av
  dagens tjenesteleverandørkrav slik jeg forstår dem.
- Egen SMP, eller registrering i ELMA via avtale, for mottak.

**Frugalitet:** det finnes ingen moden AS4-stabel i Rust. De som brukes
(phase4, Oxalis) er Java, og en JVM-sidevogn sprenger budsjettet i
docs/frugality.md (API-et har 64 MB RSS-tak og måler 13 MB). Å skrive
AS4 + WS-Security selv er et prosjekt på størrelse med alt annet i
denne lista til sammen, og sertifiseringen er kalenderstyrt.

**Konklusjon: ikke mulig innen 1.1.2027, og ikke frugalt.** Vurderes på
nytt bare hvis dokumentavgiftene til en leverandør blir en reell
kostnadspost (se §6, beslutning 2).

### 2.2 Sertifisert leverandør med API (anbefalt)

regnmed blir et **sluttbrukersystem** som kobler seg til et eksisterende
aksesspunkt — en vei DFØ selv beskriver. Leverandøren eier AS4,
sertifikat, Schematron-validering ved innsending og ofte SMP-
registrering for mottak. regnmed får:

- én REST-klient i `regnmed-gov` (samme form som `stripe`: håndrullet
  over `reqwest`, som allerede er avhengighet) — **ingen nye crates**,
  ingen ny tjeneste, ingen JVM;
- én webhook (samme mønster som Stripe-webhooken: signert, idempotent,
  logget);
- en avtale, en API-nøkkel og en dokumentpris.

**Kandidater å vurdere — ikke valgt.** Leverandører med dokumentert
REST-API for systemleverandører, funnet 2026-11-13: Unit4 Access Point,
Maventa (Visma), Tickstar (Galaxy Gateway), Storecove, Pagero og
Ademico. DFØ lenker til OpenPeppols liste over sertifiserte
tjenesteleverandører. Kravene leverandøren må måles mot:

| Krav | Hvorfor |
| --- | --- |
| Partner-/flerkunde-API: mange avsendere (våre kunder) under én avtale, opprettet maskinelt | hver regnmed-kunde er sin egen juridiske avsender |
| Oppslag av mottaker per dokumenttype | ELMA-regelen avgjøres per dokumenttype (§1.2) |
| Idempotensnøkkel på innsending | omsending etter tapt svar skal ikke bli to fakturaer |
| Synkron eller tidlig validering med lesbare feil | XSD ≠ Schematron (§3.4) |
| Webhook for status, MLR og fakturasvar (IMR) | status skal ikke polles |
| Mottak (C3) med SMP-registrering i ELMA via API | 2030-plikten, og kunder som vil motta før det |
| Testmiljø mot Peppol-testnettet | integrasjonstesten kjører mot det |
| Data lagret i EØS + databehandleravtale | forskriften § 7-5 og docs/kundevilkar.md §8 |
| Pris per dokument og fast månedspris | prisvedtaket 49/99 kr/mnd inkluderer «alt» (§6) |

## 3. Integrasjonen i regnmed

### 3.1 Flyten

```
Faktura utstedt (uendret: nummer, bilag, PDF lagret i én tx)
  │
  ├─ GET …/invoices/{iid}/delivery     ← oppslag: hvilken kanal gjelder?
  │      { "kanal": "peppol" | "epost", "grunn": "...", "mottaker": "0192:…" }
  │
  └─ POST …/invoices/{iid}/send        ← SAMME endepunkt som i dag
         kanal = peppol  → oppslag, EHF rendres og lagres, utsendelsesrad
                           skrives i ÉN tx, jobb legges i køen → 202
                           arbeideren sender til leverandøren
         kanal = epost   → som i dag: PDF på e-post via regnid-railen
         begge           → én rad i `utsendelse`
```

**Leverandørkallet går ikke i forespørselen.** Forespørselen gjør det
som er raskt og må avgjøres før svaret — oppslaget (kanalvalget),
rendringen, lagringen av XML-en og utsendelsesraden — i én transaksjon,
og legger så en jobb i den eksisterende JetStream-arbeidskøen
(docs/jobs.md) med utsendelses-id-en. `regnmed work` gjør HTTP-kallet
til leverandøren med samme id som idempotensnøkkel og prøver på nytt ved
nettverksfeil. Da er rekkefølgen entydig (raden finnes før noe er
sendt, akkurat som e-postraden finnes før meldingen er levert), en treg
leverandør holder ikke en forespørsel åpen, og «ble den sendt?» besvares
av hendelsene i §3.3, ikke av om forespørselen rakk å svare.

Sending forblir **uttrykkelig menneskelig handling** (docs/faktura.md),
og `Right::InvoiceSend` vokter fortsatt. Ingen ny rettighet: kanalen er
en konsekvens av mottakeren, ikke et valg som krever egen tillatelse.

### 3.2 Oppslaget (ELMA/SMP)

- Deltaker-id er `0192:<kjøperens orgnr>` — samme som `EndpointID` i
  dokumentet. Ingen orgnr → ikke Peppol-adresserbar → e-post (og fra
  2028: advarsel, §1.4).
- Oppslaget går til **leverandørens** oppslags-API i fase 1: det svarer
  på nøyaktig det som betyr noe — om *dette* aksesspunktet kan levere
  *denne* dokumenttypen. Et eget SML/SMP-oppslag (offentlig, uten avtale)
  er en mulig senere forenkling, ikke en forutsetning.
- Oppslaget gjøres **ved hver sending**, ikke fra en cache: en
  registrering kan komme og gå, og en feil kanal er et lovbrudd i den
  ene retningen og en tapt faktura i den andre. Resultatet vises også på
  partsiden («Mottar EHF» / «Ikke i ELMA», med tidspunkt), som visning,
  ikke som avgjørelse.
- **Oppslaget svarer ikke → ingen sending.** Portalen sier fra. Å falle
  tilbake til e-post når vi ikke vet, kan bryte plikten.

### 3.3 Én utsendelseslogg

`utsendelse` (migrasjon 0020) er innsettings-bar, og id-en er
meldingens id. Foreslått migrasjon (0020 endres ikke, den er applyert;
nye kolonner og tabeller får engelske navn):

```sql
alter table utsendelse
    add column channel text not null default 'epost'
        check (channel in ('epost', 'peppol')),
    add column peppol_receiver text,          -- '0192:<orgnr>'
    add column attachment_id uuid references attachment (id),
    alter column to_email drop not null;
-- riktig adressefelt per kanal; den gamle sjekken (to_email <> '') står
alter table utsendelse add check (
    (channel = 'epost'  and to_email is not null and peppol_receiver is null) or
    (channel = 'peppol' and peppol_receiver is not null and attachment_id is not null));

create table dispatch_event (
    id             uuid primary key,
    dispatch_id    uuid not null references utsendelse (id),
    provider_event text not null unique,   -- webhookens dedup-nøkkel
    kind           text not null,
    detail         text not null default '',
    received_at    timestamptz not null default now()
);  -- innsettings-bar, samme triggere som utsendelse
```

XML-en refereres gjennom vedleggsraden (§3.5), som alt bærer SHA-256 og
hash-sjekkes ved lesing — utsendelsesraden lagrer ingen egen kopi av
hashen.

**Leserne av `to_email` må følge med i samme endring.**
`regnmed_db::DispatchRow` dekoder kolonnen som `String`, og
`list_dispatches` (og dermed `GET …/invoices/{iid}/utsendelser`, feltet
`to` i svaret) ville feile på første Peppol-rad. Feltet blir
`Option<String>`, svaret får `kanal` og `to` blir mottakeradressen for
kanalen (e-post eller Peppol-id), og portalens utsendelsesliste viser
begge. Integrasjonstesten må ha en Peppol-rad i lista.

Leverandørens **idempotensnøkkel = utsendelses-id-en**, slik
`Nats-Msg-Id` er det for e-post og faktura-id-en er det mot Stripe.
Logg-raden og innsendingen er samme hendelse; en omsending etter tapt
svar blir en no-op hos leverandøren.

**Status er beregnet, aldri lagret** — samme lære som abonnement og
agentoppgaver. Den innsettings-bare hendelsestabellen over
(dispatch_event) skrives av webhooken:

| `kind` | Kilde | Betyr |
| --- | --- | --- |
| `akseptert` | leverandøren | validert og tatt imot av C2 |
| `avvist` | leverandøren | Schematron-/valideringsfeil — **detail** bærer regelen og teksten |
| `levert` | AS4-kvittering via leverandøren | fremme hos kjøperens aksesspunkt |
| `mlr` | Peppol MLR | kjøperens system avviste/godtok meldingen |
| `svar` | Peppol Invoice Response (IMR) | kjøperen har akseptert, bestridt, betalt … |

Webhooken dedupliserer på leverandørens hendelses-id (unik kolonne,
som `payment_intent_id` på kortbetaling, migrasjon 0043). `GET …/invoices/{iid}/utsendelser`
viser siste hendelse per rad. En `avvist` er **ikke** en feil som
svelges: den vises i fakturalisten og purre-/oppfølgingsvisningen, med
regel-id og leverandørens tekst, oversatt der vi kjenner regelen.

### 3.4 Validering: XSD er ikke Schematron

docs/ehf.md sier det ærlig: xmllint mot UBL 2.1 XSD beviser velformet
UBL, ikke at PEPPOL BIS-reglene holder. Tre lag:

1. **Før utstedelse, i Rust** — de reglene som avhenger av *data*, ikke
   av rendreren, sjekkes der dataene kommer inn, fordi en utstedt faktura
   ikke kan endres: å rette en avvisning etter utstedelse betyr kreditnota
   og ny faktura. Kjøperens orgnr, adresse og landkode, og
   **kjøperreferanse** (se under).
2. **I CI** — den offisielle Schematron-en for PEPPOL BIS Billing 3.0
   (EN16931 + PEPPOL + de norske reglene) vendores i `docs/ehf/` med
   versjon og dato, og kjøres mot en matrise av rendrede dokumenter
   (faktura, kreditnota, valuta, nullsats, flere satser, uten KID).
   Schematron-ene er XSLT 2.0, så CI trenger en XSLT 2-prosessor (Saxon
   HE) — **bare i CI**, aldri i drift, som xmllint i dag. Peppol slipper
   nye regelversjoner to ganger i året med fast ikrafttredelse; det legges
   på regelverksrevisjonens sjekkliste (docs/regelverk.md).
3. **Ved innsending** — leverandøren validerer; en avvisning blir en
   `avvist`-hendelse med regelen navngitt (§3.3).

**Funn: kjøperreferansen.** BIS 3.0 krever kjøperreferanse (BT-10) eller
ordrereferanse (BT-13) (regel PEPPOL-EN16931-R003). I dag fyller
`regnmed-db::ehf` BT-10 med **vårt eget kundenummer** (`party_no`). Det
består valideringen, men er feil innhold: offentlige og mange større
kjøpere ruter fakturaen internt på «Deres referanse», og en faktura med
vårt kundenummer der havner i en feilkø hos dem. Fase 1 trenger et felt
«Deres referanse / bestillingsnummer» på fakturaen (og standardverdi på
kunden), satt ved utstedelse.

### 3.5 Lagring av det som ble sendt

`invoice_ehf` rendrer fra fakturaens låste rader, **men også fra
kundens nåværende kontaktdata** (adresse, e-post — redigerbare, ikke
hashet). Dokumentet er derfor ikke byte-identisk «for alltid» hvis
kunden flytter, og det vi rendrer i morgen er ikke nødvendigvis det vi
sendte i dag.

Derfor: **XML-en som sendes, lagres** — som bilagsvedlegg på
fakturabilaget i objektlagringen (docs/blob-store.md, innholdsadressert,
hash sjekket ved lesing), og utsendelsesraden peker på vedlegget. En
omsending sender de **lagrede** bytene, ikke en ny rendring. Det oppfyller
§ 13 annet ledd hvis den gjelder selgerens kopi (§1.5), og det gjør
«hva sendte vi?» til et oppslag i stedet for en rekonstruksjon.

Det endrer en avgjørelse i docs/ehf.md («EHF-en lagres ikke som
vedlegg … to sannheter»). Den holdt så lenge EHF-en bare var en
nedlastbar konvolutt; når den *sendes*, er den salgsdokumentet kjøperen
mottok. PDF-en og XML-en er da to representasjoner som begge er sendt,
utledet av de samme låste radene. ehf.md oppdateres når dette bygges.

### 3.6 Avsenderen som Peppol-deltaker

Peppol-reglene krever at aksesspunktet kjenner avsenderen sin.
Hver regnmed-kunde som skal sende EHF, opprettes derfor som avsender hos
leverandøren — maskinelt, første gang selskapet sender, med orgnr og
navn fra Enhetsregisteret (vi onboarder allerede derfra). MLR og
fakturasvar adresseres til **avsenderens** Peppol-id; om selskapet må
være SMP-registrert for å motta dem, avhenger av leverandøren og må
avklares i valget.

**Mottak er en egen beslutning.** Å registrere et selskap som mottaker av
faktura i ELMA betyr at leverandørene deres **må** sende EHF dit — fra
den dagen. Det kan ikke skje i stillhet. Fase 3 gjør det som et
uttrykkelig valg per selskap, med innkommende EHF inn i bilagsinnboksen
samme vei som e-post-inn (docs/epost-inn.md): logget, dedup på
dokument-id, aldri stille forkastet. En kunde som kommer fra et annet
system, er ofte alt registrert der; registreringen må flyttes (avregistrering
hos det gamle aksesspunktet først).

## 4. Fasene

**I dag er det 7 uker til 1.1.2027.** Den kritiske stien er
leverandøravtalen, ikke koden.

### Fase 0 — nå (operatøren, uke 47–48)

- Velge leverandør (§2.2-tabellen), få tilbud, signere avtale +
  databehandleravtale. Testkonto mot Peppol-testnettet.
- Bekrefte datalagring i EØS og legge leverandøren til som
  underdatabehandler i docs/kundevilkar.md §8 — **30 dagers varsel** til
  eksisterende kunder står i vilkårene. Varselet må ut i god tid før
  1.1.2027, eller utsendelsen kan ikke slås på for dem som protesterer.
- API-nøkkel og webhook-hemmelighet i klusterets secrets, aldri i repoet
  (docs/secrets.md), adressene registrert i docs/secrets.md.

### Fase 1 — minimum for 1.1.2027 (kode)

1. `regnmed-gov`-klient: opprett avsender, slå opp mottaker per
   dokumenttype, send dokument (idempotent), verifiser webhook.
2. `GET …/invoices/{iid}/delivery` + kanalvalg i `POST …/send`;
   oppslag som feiler stopper sendingen; leverandørkallet i
   arbeidskøen (§3.1).
3. Migrasjonen i §3.3 (kanal, Peppol-mottaker, vedleggsreferanse,
   hendelsestabell), leserne av `to_email` oppdatert; webhook-endepunkt.
4. Sendt XML lagret som vedlegg; omsending sender lagrede byte.
5. «Deres referanse» på faktura (BT-10) og standard på kunden.
6. Schematron i CI (§3.4 lag 2), og de dataavhengige reglene sjekket før
   utstedelse.
7. Portal: «Send» viser kanalen før klikket («Sendes som EHF til
   Kunde AS (0192:…)»), statuslinje med avvisningsgrunn, mottar-EHF-merke
   på kunden.
8. **Driftsselskapets egne abonnementsfakturaer** (`abonnement-oppfolging`
   steg 1) går gjennom samme kanalvalg — BogenTech AS er selv
   bokføringspliktig selger til bokføringspliktige kjøpere, og plikten
   gjelder oss fra samme dag. Jobben er uten menneske, så «oppslaget svarer
   ikke» (§3.2) trenger en egen regel: fakturaen står usendt, jobben
   rapporterer den slik den i dag rapporterer et selskap uten
   e-postadresse, og neste kjøring prøver igjen. **Purretrappen starter
   ikke** for en faktura uten utsendelsesrad — ellers regnes påminnelse og
   gebyr fra et forfall på en faktura kunden aldri fikk.
9. Integrasjonstest mot leverandørens testmiljø (skipper uten nøkkel,
   som nats-testene), med payload **fanget fra ekte leveranser** —
   Stripe-lærdommen (CLAUDE.md punkt 56): payloader skrevet ut fra hva
   koden leser, bekrefter bare kodens egen antakelse.

Det som ikke er med i fase 1, er bevisst: fakturasvar vises bare som
hendelse, mottak er av, ingen eget SMP-oppslag.

### Fase 2 — Q1 2027

- MLR og fakturasvar (IMR) tolket og vist som forståelig status;
  «betalt»-svar er informasjon, aldri bokføring (bankavstemmingen
  bokfører).
- Kreditnota-kanalen når mottakeren bare tar imot faktura (§1.5).
- Eget SML/SMP-oppslag hvis leverandørens oppslag er tregt eller koster.

### Fase 3 — 2027 (frivillig mottak)

- Per selskap: «Motta EHF fra leverandørene dine» → SMP-registrering via
  leverandøren, innkommende dokumenter til bilagsinnboksen (originalen
  lagres, forslaget regnes ut — docs/ehf.md «Inn»).
- Flytting av eksisterende ELMA-registrering fra forrige system.

### Fase 4 — før 1.1.2028

- Kjøperens orgnr påkrevd ved salg til virksomhet (§ 5-1-2).
- Salgsdokumentet skal alltid sendes (§ 5-2-1 fjerde ledd): «usendte
  fakturaer» som egen visning og som revisjonskontroll. `abonnement-oppfolging`
  sender også betalte abonnementsfakturaer (rettet i #223,
  docs/abonnement.md §5.3).

### Fase 5 — før 1.1.2030

- Mottak obligatorisk: alle selskaper registrert som mottakere (med
  § 1-2-unntaket for lav omsetning som det eneste flagget, §1.3).

## 5. Funn underveis som ikke venter på denne saken

- **BT-10 = vårt kundenummer** (§3.4) — rettes i fase 1, men gjelder
  også EHF-en som lastes ned i dag.
- ~~**Kortbetalte abonnementsfakturaer sendes ikke**~~ — rettet i #223:
  oppgjorte abonnementsfakturaer og kreditnotaene deres sendes nå, og
  e-posten sier om fakturaen er betalt eller kreditert.
- **EHF-en rendres fra kundens nåværende kontaktdata** (§3.5) — «byte-
  identisk for alltid» i docs/ehf.md stemmer bare så lenge kunden ikke
  endrer adresse.

## 6. Avgjørelser operatøren må ta

1. **Leverandør.** Hvilken av kandidatene (eller andre) — målt mot
   tabellen i §2.2. Dette er den kritiske stien.
2. **Hvem bærer dokumentprisen.** Prisvedtaket (docs/abonnement.md) sier
   at alt er inkludert i 49/99 kr/mnd. Inkludere EHF (enkelt, men
   kostnaden vokser med kundens volum), eller et tak/tillegg. Avgjør også
   når eget aksesspunkt eventuelt blir verdt å se på igjen.
3. **Lagre sendt XML** (§3.5) — anbefalt; reverserer en avgjørelse i
   docs/ehf.md.
4. **Mottak:** frivillig per selskap i 2027 (anbefalt) eller ikke før
   2030.
5. **Hva hvis fase 1 ikke er i drift 1.1.2027.** Plikten er kundenes, og
   det finnes ingen innebygd utsettelse. Kundene må i så fall varsles om
   at fakturaer til ELMA-registrerte kjøpere ikke kan sendes lovlig fra
   regnmed før utsendelsen er på plass.
