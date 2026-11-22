# Merverdiavgift: codes, rates, spesifikasjon, melding

## Standard codes

regnmed uses **Skatteetaten's SAF-T standard VAT codes directly** as its
own codes (`vat_code` table, complete list per
`docs/saft/Standard_Tax_Codes.csv`). There is no internal→standard
mapping to maintain or get wrong; SAF-T's `StandardTaxCode` and the
mva-melding's `mvaKode` are our codes verbatim.

Every code has a **rate class**: `regular` (25 %), `middle` (15 %,
næringsmidler), `low` (12 %), `raw_fish` (11,11 %), `zero`. A code is a
stable identity; a rate is a dated fact.

## Registreringen avgjør om en kode i det hele tatt kan brukes (#99)

Utgående mva kan bare kreves inn av en registrert selger (mval. §15-11
andre ledd), og inngående mva bare fradragsføres av en registrert
kjøper (§8-1). Registreringsstatusen er lagret og datert i
`company_registrering` (§5-1-2, #81 — se docs/faktura.md); fram til #99
STYRTE den bare påtegningene på dokumentet, mens beløpene gikk sin
egen vei. Et uregistrert selskap kunne altså utstede en faktura med
25 % mva og fradragsføre inngående mva på et utlegg, uten innsigelse.

Vakten er `settings::require_vat_registered`, slått opp på **dokumentets
dato** — aldri dagens, ellers endrer et gammelt bilag seg når noen
laster det ned på nytt. Den ligger i bokføringsveien, ikke i portalen.

**Siden 2026-11-13 sitter den også i `post_voucher_in`**, den ene veien
alle bilag går inn i hovedboken: et bilag med en kode som krever
registrering avvises når selskapet ikke var registrert på
bilagsdatoen. Før det spurte bare faktura-, tilbuds- og utleggsveien
(tabellen under), mens innboksen (Bilag-skjemaet foreslår kode 1),
manuelle bilag og kassaoppgjøret bokførte koden uten innsigelse — og
beløpene havnet i spesifikasjonen og meldingen. To unntak:
importert historikk (journal `IMP`; registreringen ligger før regnmed,
og kildefilen er beviset) og en tilbakeføring (den opphever en
postering som ble kontrollert da den ble gjort, også om selskapet
siden er avregistrert). Testet i `tests/grupper/hovedbok.rs`
(`vat_codes_need_a_registration_on_every_posting_path`). Veiene som
spør selv, gjør det fortsatt — de gir en tidligere og mer presis
beskjed:

| Vei | Hva som skjer uten registrering |
| --- | --- |
| faktura, kreditnota, repeterende (`invoice::resolve_lines`) | nektes |
| tilbud og ordre (`create_sales_document`, `update_quote`) | nektes der linjene SKRIVES |
| utlegg (`approve_expense`) | nektes; hele brutto er kostnaden |
| abonnementsskinnen (`abonnement::utgaende_mva`) | velger kode `7` selv |

Kodene `0`, `6` og `7` slipper alltid gjennom: de sier at transaksjonen
ligger *utenfor* mva-systemet, og det er nettopp det et uregistrert
selskap skal si. Kode `5` gjør det ikke, selv om den også er nullsats —
den betyr en REGISTRERT selgers fritatte omsetning (mval. kap. 6), en
påstand et uregistrert selskap ikke kan fremsette. Forskjellen er null
kroner og to helt ulike utsagn i hovedboken og SAF-T-en.

Et uregistrert selskap fakturerer altså uten kode. Den inngående
avgiften det ikke får fradrag for er en reell kostnad, og skal derfor
dekkes av prisen — men prisen er bare en pris, og kan ikke føres opp
som avgift.

## Dated rates

`vat_rate` (migration 0006) stores rates in **basis points** with
validity dates: e.g. lav sats 10 % from 2016, 12 % from 2018-01-01, 6 %
during covid (2020-04-01 → 2021-09-30), 12 % since. All beregning —
reports, SAF-T tax lines, mva-melding — resolves the rate **valid on the
voucher date**. `vat_code.rate_percent` is informational only ("current
rate") and is never used in computation. History before 2016 is out of
scope; a voucher older than the rate table fails loudly rather than
computing wrongly.

## Beregning rules

In `regnmed-core::mva` (pure, no I/O):

- `vat_of_base(base_ore, rate_bp)` — VAT from grunnlag, integer øre,
  rounded half away from zero, sign-preserving.
- `split_gross(gross_ore, rate_bp)` — splits a VAT-inclusive amount so
  that `base + vat == gross` exactly (vat is the remainder).
- Terminer are the standard two-month periods (1 = januar–februar … 6 =
  november–desember); `Termin::of/start/end` own the boundary math.
- **Terminordninger** (#51): to-måneder is the default; **årstermin**
  (omsetning under grensen, etter søknad) and **primærnæring** are
  yearly. The ordning Skatteetaten has GRANTED is recorded per company
  with `valid_from` (migration 0028, append-only history with the
  vedtaksreferanse) — eligibility is never auto-detected. `Terminordning`
  owns the ordning-aware periode math and the **leveringsfrister**
  (skatteforvaltningsforskriften §8-3: 1 måned og 10 dager, med
  særregelen 31. august for 3. termin; årstermin 10. mars; primærnæring
  10. april). Spesifikasjonen, mva-meldingen (skattleggingsperiodeAar
  for the yearly ordninger — the schema's own distinction) and the
  portal picker all follow the company's ordning; a periode number
  outside it is refused. Kortere terminer ved restanse are pålagt
  individually and deliberately out of scope until asked.

Ledger sign convention throughout: positive = debit. Sales bases are
credits (negative), purchase bases debits (positive).

## Mva-spesifikasjon (`regnmed mva-report`)

Per termin (or year): for each code and rate actually used, the summed
grunnlag and the **beregnet** avgift (`vat_of_base` on the sum). Beregnet
— not posted — because comparing it against the balance of the posted
VAT accounts (2700/2710) is precisely the accountant's control. A period
spanning a rate change shows one line per rate. The summary derives
utgående (codes 3, 31, 32, 33), inngående fradrag (1, 11, 12, 13, 14,
15) and netto å betale / til gode.

## Mva-melding (`regnmed mva-melding`)

Built in `regnmed-core::mvamelding` from the same spesifikasjon lines,
rendered as `mvaMeldingDto` XML per Skatteetaten's published XSD
(vendored in `docs/mva-melding/`, validated on every test run and in CI).

Conversion rules — all in one place, tested:

| Ledger | Melding |
| --- | --- |
| signed øre, positive = debit | **whole kroner**, rounded half away from zero |
| utgående avgift is a credit (negative) | signed by effect on payable: utgående **positive**, fradrag **negative** |
| grunnlag on every coded line | grunnlag + sats only on utgående/omsetning codes; fradrag codes report only `merverdiavgift` |
| code 0 postings exist | code 0 is **not reported** in the melding |

`fastsattMerverdiavgift` = the sum of all line effects, which is the same
netto the mva-report shows.

### Omvendt avgiftsplikt og innførsel er TOSIDIG (#82)

Kjøperen både beregner utgående avgift og fører fradraget for den —
mval. §11-1 (2) og (3), jf. §3-30. Fram til 2026-08-07 sendte vi bare
den beregnede siden, og `fastsattMerverdiavgift` krevde da inn 25 % av
et grunnlag kjøperen ikke skyldte noe på.

Hvilke koder som bærer fradraget er **ikke vår vurdering**. Det står per
kode i Skatteetatens egen kodeliste (*Norwegian SAF-T Standard VAT/Tax
codes*, dokumentet mva-melding-XSD-en selv peker til for `mvaKode`).
Ordrett for kode 81: «Grunnlaget og beregnet utgående
innførselsmerverdiavgift føres i post 9, mens fradragsberettiget
inngående innførselsmerverdiavgift føres i post 17». For kode 82 stopper
den samme setningen etter post 9.

| Koder | Kodelisten sier | Virkning på fastsatt |
| --- | --- | --- |
| 81, 83, 86, 88, 91 | beregnet utgående **og** fradrag | **null** |
| 82, 84, 87, 89, 92 | bare beregnet utgående | hele avgiften |
| 14, 15 | bare fradraget (avgiften betalt ved innførsel) | fradrag |
| 20, 21, 22 | kostnadsmarkør på fakturaen | **rapporteres ikke** |

Postnumrene tilhører gamle RF-0002; tosidigheten de beskriver er en
egenskap ved KODEN og følger med inn i den kodebaserte meldingen.

Kostnadsmarkørene var tidligere rutet sammen med 8x/9x-kodene, slik at
en innførsel beregnet avgift to ganger — én gang under kode 21 og én
gang under kode 81. De hoppes nå over som kode 0. Kodelisten sier selv
at de ikke er obligatoriske, og at «ved selve avgiftsberegningen
benyttes kode 81 eller kode 14».

### Avgiften bokføres (#82)

Beregnet avgift på omvendt avgiftsplikt og innførsel fantes bare som et
tall i rapporten. XSD-en kaller `merverdiavgift` «Bokført beløp for
merverdiavgift» — vi rapporterte altså et bokført beløp som ikke var
bokført noe sted, og bokføringsforskriften §3-1 nr. 8 krever at
merverdiavgiften spesifiseres i regnskapet.

Utvidelsen skjer i POSTERINGSTRANSAKSJONEN (`post_voucher_in` →
`expand_reverse_charge`), ikke i den enkelte inngangen: da kan ingen vei inn i
hovedboken glemme den, og linjene hashes sammen med resten av bilaget.
Satsen er den som gjaldt på BILAGSDATOEN, fra samme daterte tabell som
spesifikasjonen — hovedboken og meldingen regner på samme kilde.

| Kodetype | Linjer som legges til | Kostnaden |
| --- | --- | --- |
| med fradragsrett | 2701 kredit, 2711 debet | uendret |
| uten fradragsrett | 2701 kredit, **grunnlagskontoen** debet | øker |

Begge summerer til null, så et bilag som balanserte før balanserer
etter. Ikke-fradragsberettiget avgift er en del av hva tingen kostet,
derfor havner den på grunnlagskontoen og arver dens avdeling/prosjekt.
Kontoene 2701/2711 opprettes ved behov, som resten av kontoplanen. De
tillagte linjene er UKODEDE, akkurat som fakturamotorens mva-linje —
en kode der ville telt det samme grunnlaget to ganger.

**Gjenstår, uavklart mot Skatteetaten:** om en tosidig kode skal sendes
som ÉN linje (beregnet avgift i `merverdiavgift`, fradraget bare i
totalen — slik vi gjør nå) eller som to linjer. XSD-en tillater begge,
og `fastsattMerverdiavgift` er uansett riktig. Spørsmålet avklares mot
valideringstjenesten før første innsending (`regnmed mva-melding
--validate`, docs/gov.md). Selve avgiftsbeløpet er ikke lenger avhengig
av svaret — bare linjeformen.

Validation and submission against Skatteetaten's APIs: see
[gov.md](gov.md).

## Godkjenn og send (ID-porten + Altinn)

En mva-melding må kunne tilskrives en **person**: Skatteetaten godtar ikke
Maskinporten eller systembruker for validering og innsending
(docs/gov.md, SSV-5760). Alt annet gjør systemet, men sendingen skjer ved
at en person med rett til det godkjenner akkurat denne meldingen med
ID-porten. Det er ikke en innlogging i regnmed: brukeren beholder sin
vanlige innlogging, og godkjenningen gjelder én innsending.

**Flyten:**

1. Portalen sier `POST …/vat-filings {year, period}` (rettigheten
   `MVA_SEND`, godtatte kundevilkår, terminen må være over). Serveren
   bygger meldingen fra hovedboken og lagrer den med SHA-256 **før** noe
   sendes. Raden endres aldri.
2. Personen ser termin og beløp, og sendes til ID-porten med PKCE (S256),
   `state`, `nonce`, `acr_values=idporten-loa-high` og `prompt=login`. En
   gammel ID-porten-økt er ikke en godkjenning av denne meldingen.
3. ID-porten sender nettleseren til `/mva-godkjenning`. **Den adressen
   godkjenner ingenting.** Portalen poster `code` og `state` med personens
   egen bearer-token til `…/approve`, og serveren krever samme person som
   startet, en `state` som bare virker én gang, og uendret melding. Slik kan
   ingen lure en kollega til å godkjenne en innsending kollegaen ikke
   startet.
4. Serveren veksler koden og verifiserer id_token: utsteder, mottaker,
   nonce, signatur, sikkerhetsnivå og at `auth_time` ikke er eldre enn dette
   forsøket (60 sekunders klokkeslingring). Under periodelåsen avgjøres det
   på nytt om innsendingen kan godkjennes, siden den kan ha blitt erstattet
   mens personen var hos ID-porten. Godkjenningen, det forseglede tokenet og
   jobben `vat_filing_submit` lagres i samme transaksjon.
5. **Arbeideren** gjør alle utgående kall: veksler til Altinn-token, spør
   Altinn hvem personen er, kaller Skatteetatens validering (avvik stopper
   innsendingen og vises), oppretter instansen, laster opp konvolutt og
   melding, og tar `process/next` to ganger. Regnskapet sjekkes på nytt rett
   før instansen opprettes og rett før det bekreftende steget.
6. Betalingsinformasjon, kvittering og valideringsresultat hentes uten at
   personen er til stede. Arbeideren spør hvert tiende sekund så lenge
   Altinn-tokenet lever. I TT02 levde tokenet to minutter. Kommer svaret
   senere, står innsendingen som `sendt`, og portalen tilbyr «Logg inn og
   hent svar».

**Hva som lagres om personen:** regnmed-personen, ID-portens `sub`
(parvis, ikke fødselsnummer), `acr`, `auth_time` og navnet Altinn har på
personen. **Fødselsnummeret tas aldri vare på.** `pid` leses ikke ut av
tokenet. Altinn-profilen inneholder `ssn`, men parseren henter bare navnet, og
en feil fra profilen logges bare med statuskoden. Navnet hentes fra Altinn
fordi ID-porten bare oppgir navn for noen eID-er. Det er dette navnet som står
i konvolutten og i `godkjent_av`, ikke navnet på regnmed-kontoen. Kan ikke
Altinn oppgi navnet, stopper innsendingen før noe er sendt inn. En gjenopptatt
innsending kan da ha et utkast i Altinn. Bekrefter en annen person enn den
konvolutten navngir, slettes utkastet og ingenting sendes. Tokenet ligger
forseglet med
`IDPORTEN_SEALING_KEY` i `vat_filing_grant`, og raden slettes når arbeidet
er ferdig eller tokenet er utløpt.

**En regnmed-bruker er bundet til én ID-porten-identitet.** Første
godkjenning lagrer `sub` per (person, utsteder, klient) i
`idporten_identity`. Logger samme bruker senere inn som en annen person,
nektes godkjenningen med en forklaring. Tabellen er versjonert og
innsettings-bar, og nyeste rad gjelder. Må en binding løses, gjør en operatør
det med skriftlig grunnlag:

```sh
regnmed idporten-rebind --person <id> --issuer <utsteder> --client-id <klient> \
  --operator "<navn>" --reason "<sak og grunnlag>"
```

Kommandoen legger til en rad med hvem og hvorfor. Den gamle bindingen består
som historikk, og neste innlogging binder personen på nytt.

**En usikker innsending gjentas aldri i blinde.** Instansens id lagres før
første opplasting, og en ny kjøring leser hvor instansen faktisk står og
fortsetter derfra. `sendt` skrives bare når Altinn viser det Skatteetaten
dokumenterer: oppgaven `Tilbakemelding`, eller en avsluttet prosess. Alt
annet blir `uavklart`. Én kjøring om gangen holder en advisory-lås per
innsending på en egen tilkobling, og hver skriving kontrollerer at historikken
ikke har fått noe nytt siden, så et sent svar aldri overskriver en nyere
tilstand. Rett før hvert kall som endrer noe i Altinn kontrollerer kjøringen
at den fortsatt holder låsen og at historikken er uendret. En jobb som møter en
annen kjøring venter en stund, og går så tilbake i køen uten å bruke et forsøk,
så den aldri står som ferdig mens den andre arbeider.

Et token som ikke kan brukes sier ingenting om hva som er sendt. Uten instans
gir det `ma_godkjennes`. **Med en instans på plass blir det alltid `uavklart`**,
og portalen ber personen sjekke Altinn før noe annet gjøres. Etter et usikkert
steg lukkes en innsending som `avbrutt` bare når Altinn har bekreftet at utkastet
er slettet. Svarer ikke slettingen, forblir den `uavklart`.

Valideringen regnes som ren bare når svaret faktisk er et `valideringsresultat`
med `ingen avvik`. Et avkortet eller ukjent svar stopper innsendingen.

Status beregnes fra de innsettings-bare hendelsene:

| Status | Betyr | Neste steg |
| --- | --- | --- |
| `opprettet` | lagret, ikke godkjent | Godkjenn og send |
| `godkjent`, `oppretter_instans`, `instans_opprettet` | arbeideren jobber. Uten åpen jobb har den stoppet | vent, eller logg inn og fortsett |
| `avvik` | Skatteetatens kontroll stoppet den | rett og godkjenn på nytt |
| `feilet` | Altinn avviste steget, oftest manglende rett | logg inn og fortsett |
| `ma_godkjennes` | tokenet kunne ikke brukes | logg inn og fortsett |
| `uavklart` | vi vet ikke hvor langt den kom | sjekk Altinn. Med instans: logg inn, så leses den. Ellers, eller hvis den er avklart i Altinn: lukk med notat |
| `lukket_etter_kontroll` med funn `levert` | levert ifølge personens kontroll i Altinn. regnmed har ikke sett kvitteringen selv | ingen, terminen er lukket |
| `lukket_etter_kontroll` med funn `ikke_levert` | ikke levert ifølge personens kontroll | ny innsending |
| `sendt` | innsendt, venter på svar | hentes automatisk |
| `kvittert` / `avvist` | Skatteetatens svar. `avvist` krever et valideringsresultat med feil, ellers ventes det på kvitteringen | endelig |
| `avbrutt` | ikke sendt | ny innsending |

**Lukking etter kontroll** (`POST …/{fid}/close-unresolved`) krever
`MVA_SEND`, som ingen maskin kan ha, en bekreftelse på at Altinn er sjekket, et
uttrykkelig funn (`funn`: `levert` eller `ikke_levert`) og et notat. Hvem, når,
funnet og notatet lagres som en egen hendelse og vises i `lukking`. Lukkingen
endrer ingenting i Altinn. Bare `ikke_levert` åpner for en ny innsending. Med
`levert` forblir terminen lukket, og den vises som levert *ifølge personens
kontroll*, tydelig adskilt fra en kvittering regnmed har hentet selv.

**Hvem som står bak en innsendt melding** er personen på hendelsen `bekrefter`,
altså den som faktisk sendte det bekreftende steget. En senere innlogging som
bare leser instansen, endrer ikke `godkjent_av`.

**Et bekreftende steg uten klart svar sendes aldri på nytt.** Bare at Altinn
avviser akkurat det kallet (`bekreftelse_avvist`) gjør det avklart. En senere
lesing som får 401 eller 403, eller en annen `feilet`, sier ingenting om det.
Står instansen fortsatt i bekreftelse, beviser ikke det at det første kallet er
dødt. Arbeideren forsøker da å slette utkastet, og skriver `avbrutt` bare når
slettingen er bekreftet. Ellers forblir innsendingen `uavklart`.

En instans som fortsatt står i utfylling eller bekreftelse når regnskapet
er endret, sendes ikke: arbeideren avbryter den, og personen godkjenner de
nye tallene. Det samme gjelder hvis en annen person godkjenner mens
instansen står i bekreftelse, for konvolutten der navngir den første. Den
nye personen bekrefter aldri den andres konvolutt, men starter en ny
innsending. Utkastet forsøkes slettet i Altinn, og en mislykket sletting
stopper ingenting. En termin som er sendt kan ikke sendes på nytt herfra.
Korrigert melding er ikke bygget.

### Når en regnskapsfører leverer for kunden

Regnskapsføreren godkjenner med **sin egen** ID-porten-innlogging. Det må
stemme to steder, og de kontrolleres hver for seg:

- **I regnmed:** et oppdrag som gir `bokforing`, som inneholder `MVA_SEND`.
  Revisor og lesetilgang gir den ikke, og ingen maskin eller egendefinert
  rolle kan få den.
- **I Altinn:** personen må ha rett for akkurat denne klienten. Altinn-appen
  krever `merverdiavgift` eller `regnskapsforer-med-signeringsrettighet`
  for å bekrefte. Kunden gir pakken til byrået, og byråets
  klientadministrator gir den videre til den ansatte. Pakken
  `regnskapsforer-uten-signeringsrettighet` kan fylle ut, men ikke sende.

Mangler Altinn-retten, stopper innsendingen med `feilet` og en forklaring
på nettopp dette.

**Prøvd mot TT02 2026-11-22:** en ren melding for september–oktober 2026 gikk
fra regnmed gjennom ID-porten med TestID, Skatteetatens validering og Altinn.
Skatteetaten svarte med kvittering og `ingen avvik` etter om lag 55 sekunder.
Dette er dermed bekreftet:

- scopene `openid skatteetaten:mvameldingvalidering altinn:instances.read
  altinn:instances.write` er nok.
- TestID gir `acr=idporten-loa-high`, og `prompt=login`, `auth_time` og `sub`
  virker som antatt.
- `Task_1` er utfylling, `Task_2` bekreftelse, og
  `process.currentTask.name` er `Tilbakemelding` etter innsending.
- valideringen svarer `valideringsresultat` med `avvikVedMeldingslevering` =
  `ingen avvik` for en ren melding.
- konvolutten godtas uten navnerom og med én `vedlegg`-rad.

Ikke bekreftet ennå:

- at Altinn-profilen gir navnet slik appens OpenAPI-beskrivelse viser.
  Kjøringen over brukte fortsatt regnmed-kontoens navn i konvolutten.
- at `DELETE` på en instans i bekreftelse er tillatt for personen som
  godkjente.
- at svaret normalt kommer innen tokenets levetid.
- at meldingskategorien alltid skal være `alminnelig`, også for primærnæring.
  Det er slik `regnmed-core::vat_return` skriver den i dag.

**Et testselskap må ha egen mva-plikt hos Skatteetaten.** Flagget i BRREG og
Tenor er ikke nok: valideringen svarer da at virksomheten ikke er registrert.
Daglig leder fra Tenor logger inn med TestID i `tt02.altinn.no` og søker i
`skatt-test.sits.no/web/mva/`. **Startdatoen skal være første dag i terminen
som testes.** Skatteetatens veiledning foreslår 01.01.2022, men da svarer
valideringen at det mangler meldinger for alle terminene siden.

## Web API (the product surface)

The web is the product; the CLI wraps the same crate functions for
ops/admin. Authenticated, engagement-guarded endpoints (see
[auth.md](auth.md)):

| Endpoint | Returns |
| --- | --- |
| `GET /companies/{id}/reports/mva?year=&termin=` | spesifikasjonen som JSON (heltall i øre; portalen formaterer). Hver linje har `virkning_ore` — linjens virkning på det som skal betales, positiv når den øker beløpet, etter SAMME regel som meldingen (`regnmed-core::vat::payable_effect_ore`: utgående positiv, fradrag negativ, omvendt avgiftsplikt med fradrag null, kostnadsmarkører og kode 0 ikke med) — og `retning`. `utgaende_ore`/`inngaende_ore`/`netto_ore` summeres per retning (`vat_totals`), ikke per fortegn: en termin der kreditnotaer veier tyngre enn salget gir NEGATIV utgående mva, ikke et fradrag. Skjermen og meldingen kan dermed ikke være uenige om hva som skyldes (før lot «Å betale» omvendt avgiftsplikt uten fradragsrett ligge ute, mens meldingen tok den med) |
| `GET /companies/{id}/reports/mva/postings?year=&termin=&kode=` | posteringene bak én kode i terminen (bilag, dato, konto, tekst, part, grunnlag), inntil 500 med `total` — det regnskapsføreren kontrollerer før meldingen sendes |
| `GET /companies/{id}/mva/terminordning?year=` | ordningen som gjelder for året (den som gjaldt 1. januar, som rapporten og meldingen bruker), med årets terminer og frister; uten `year` dette året |
| `GET /companies/{id}/reports/mva-melding?year=&termin=` | `mvaMeldingDto` XML download |
| `GET/POST /companies/{id}/vat-filings`, `GET …/{fid}`, `POST …/{fid}/authorize`, `/approve`, `/abandon`, `GET …/{fid}/documents/{type}` | «Godkjenn og send», se over |
| `GET /companies/{id}/reports/saft?year=` (or `from=&to=`) | SAF-T XML download; header contact defaults to the authenticated person's name |

Portalen (`#/c/{id}/mva?year=&termin=&kode=`): registreringsstatusen
(«Mva-registrert fra …», eller et tydelig «Ikke mva-registrert» med
hva det betyr) og terminordningen først; årsvelger og terminer navngitt
etter månedene; siden åpner på den siste terminen som er AVSLUTTET (den
som skal leveres). Antall terminer hentes per år, så et år med en annen
ordning viser sine egne terminer. Fristen sies i ord («Frist 10.
desember 2026, om 27 dager»). Hver kode åpner posteringene bak seg, med
lenke til bilagene, og beløpene vises slik meldingen leser dem.
Meldingskortet viser status, avvik, betalingsinformasjon og kvittering og
har «Godkjenn og send». Meldingen kan fortsatt lastes ned; SAF-T er
flyttet til Rapporter.
Terminordningsskjemaet vises bare for den som har `MVA_ORDNING_ADMIN`.

### Omvendt avgiftsplikt i spesifikasjonen (rettet 2026-11-13)

Grunnlaget for omvendt avgiftsplikt og innførsel er et KJØP, bokført
som debet, men avgiften som beregnes av det er UTGÅENDE avgift (mval.
§ 11-1) — en gjeld, kredit. Spesifikasjonen leste debetgrunnlaget som
det sto, så meldingen ble et TILGODEHAVENDE: en tjeneste fra utlandet
på 100 000 kr med kode 87 ga `fastsattMerverdiavgift` −25 000 i stedet
for +25 000. `vat_specification` oppgir nå disse linjene i utgående
konvensjon (grunnlag og avgift begge kredit), og alt nedstrøms — meldingen,
virkningen, nøkkeltallenes mva-netto — leser dem som enhver annen
utgående linje. Kjernens test hadde laget linjen for hånd med et
fortegn hovedboken aldri gir, og fanget det derfor ikke; testen i
`regnmed-db/tests/mva.rs` bygger nå meldingen fra hovedboken: kode 86
koster null, kode 87 koster 25 000.

## Where it is tested

- `crates/regnmed-core/src/mva.rs` — termin boundaries (incl. leap
  years), historical rate lookup, rounding, gross-split exactness, code
  direction.
- `crates/regnmed-core/src/mvamelding.rs` — sign/unit conversion, code 0
  exclusion, fradrag lines without grunnlag, fastsatt sum, XSD validity.
- `crates/regnmed-api/tests/grupper/vat_filing.rs` — «Godkjenn og send»
  mot en mock av ID-porten og Altinn: feil person, gjenbrukt `state`,
  forfalsket id_token, gammel innlogging, feil ID-porten-identitet, endret
  regnskap, en annen godkjenner ved bekreftelse, samtidige godkjenninger,
  kjøringer som kommer for sent eller mister låsen, mislykket tokenveksling,
  bekreftet sletting etter usikre steg, lukking med notat, operatørens
  ombinding, en ødelagt rad i oppfølgingen, avvik, utløpt token,
  usikre steg som aldri gir en andre instans, og at verken token eller
  fødselsnummer står lesbart i basen. Enhetstester i `regnmed-gov::idporten`,
  `regnmed-gov::vat_return` og `regnmed-db::vat_filing`.
- `crates/regnmed-db/tests/mva.rs` (real Postgres, also CI) — the
  spesifikasjon's numbers per termin, the 2017 historical rate (10 %),
  and the dated rate flowing into SAF-T lines.
