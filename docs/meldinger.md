# Meldinger — samtaler mellom folk som jobber sammen

Én kommunikasjonsflate for alle som jobber i samme regnskap: de ansatte i
selskapet, byråets folk, og selskapets agenter. En agent er en bruker
(#101), så den står i kontaktlisten på linje med menneskene: man skriver
til den, og den svarer.

Samtalene er **én-til-én**, som i en vanlig meldingsapp: til venstre
samtalene med den nyeste meldingen øverst, til høyre den åpne samtalen,
og «Ny melding» for å velge hvem man vil skrive til.

Migrasjon 0091 · `regnmed-db::chat` · `regnmed-api::chat` ·
`regnmed-mail::chatbus` · portalen: `#/meldinger`, knappen i hver navbar.

## 1. Hvem kan skrive til hvem

**Beregnet hver gang, aldri lagret** (`chat::contacts`), fra de samme
radene som avgjør tilgang til regnskapet:

| Jeg er … | Jeg når … |
| --- | --- |
| aktivt medlem av et selskap | de andre aktive medlemmene, selskapets egne agenter, og kontaktpersonen hos hvert byrå med aktivt oppdrag |
| aktivt medlem av et byrå | mine byråkolleger, og alle aktive medlemmer og agenter i hver klient med aktivt oppdrag |

- **Et selskap når bare sine egne agenter.** En agent er én kontakt per
  selskap den har tilgang til, fordi oppgavene den får skal inn i ett
  bestemt regnskap.
- **Byråets kontaktperson** velges per oppdrag av byråets administrator
  (Byrå → Klienter → «Kontakt for kunden»), lagret i
  `engagement_contact`. Til noen er valgt — og når ingen av de valgte
  lenger er aktive medlemmer — er byråets administratorer kontakten, så
  et oppdrag aldri står uten noen å skrive til. Bare aktive medlemmer av
  byrået kan velges.
- **Lesing krever samme rekkevidde som skriving.** Når et oppdrag eller et
  medlemskap avsluttes, forsvinner samtalen fra begge listene med en gang
  (404, som et selskap man har mistet tilgangen til). Radene står —
  meldinger er en del av sporet — men tilgangen gjør det ikke.
- Ingen nye rettigheter: rekkevidden følger medlemskap, oppdrag og
  agentens tilgang. Plattformrollene (docs/auth.md §8) gir ingen
  rekkevidde.

## 2. Modellen

| Tabell | Hva | Regel |
| --- | --- | --- |
| `chat_conversation` | én samtale; `direct_key` = de to sorterte person-id-ene (+ selskapet for en agent), unik | innsettings-bar |
| `chat_participant` | de to i samtalen, med `read_until` | bare `read_until` kan oppdateres |
| `chat_message` | det som ble sagt; `task_id` når meldingen var en ordre til en agent | innsettings-bar — en rettelse er en ny melding |
| `chat_message_document` | vedlegg = dokument i et selskaps bilagsinnboks | innsettings-bar |
| `engagement_contact` | byråets valgte kontaktpersoner per oppdrag | oppdateres av byråets admin |

To personer har nøyaktig én samtale (to for en agent i to selskaper).
Å åpne en samtale som finnes, gir den som finnes — fra hvilken side som
helst.

## 3. Å skrive til en agent er å gi den en oppgave

En melding til en agent oppretter en `agent_oppgave` (#103) i agentens
selskap, med meldingen, vedleggene og de siste ti meldingene i samtalen
som bestilling. Det finnes altså **ingen ny vei inn i hovedboken**:
oppgavekøen, workeren og agentens egen rolle er de samme som før, og
agenten gjør bare det rollen dens tillater. Attesteringspolicyen
(`krev_for_maskin`) gjelder uendret for alt den bokfører.

- Har agenten et åpent **spørsmål** (`sporsmal`) i samtalen, blir neste
  melding et SVAR på den oppgaven (en `pagar`-hendelse) i stedet for en ny
  oppgave — agenten fortsetter med hele historikken.
- Agentens svar (`sporsmal`, `utfort`, `avvist`) legges tilbake i samme
  samtale (`chat::mirror_task_answer`). Bare integrasjonen oppgaven ble
  gitt til, snakker i samtalen — avgjort på identitet, ikke navn. Et
  menneske som avslutter oppgaven i oppgavevisningen, snakker ikke i
  agentens navn.
- **En agent starter ikke samtaler og skriver ikke til andre agenter**:
  den svarer gjennom oppgavene sine. #103s egenskap («en agent skriver
  ikke sine egne ordrer») består.

**Avgjørelse, bevisst:** alle som når agenten kan be den om noe — også en
`ansatt` som selv ikke kan bokføre, og byråets folk. Det er meningen
(«bokfør denne for meg»), og det er slik en menneskelig kollega i
regnskap også jobber: den som ber, står i oppgavens spor
(`opprettet_av`), og den som utfører, gjør det med sin egen myndighet.
Et selskap som ikke vil ha det slik, gir agenten en rolle uten
bokføringsrett eller slår på `krev_for_maskin`.

## 4. Vedlegg er dokumenter i innboksen

Et vedlegg lastes opp til bilagsinnboksen i et selskap **begge jobber i**
(`POST /companies/{id}/inbox`, krever `BILAG_LAST_OPP`), og meldingen
peker på dokumentet. Et bilag hører hjemme der det kan bokføres, hashes
og oppbevares (#96) — ikke i et filarkiv ved siden av. Hvilke selskaper
det gjelder, står i samtalens `attach_to`: agentens selskap, eller
selskapene to mennesker deler. To byråkolleger deler ingen innboks, så
der finnes ikke vedleggsknappen.

Konsekvens: samme fil kan ikke legges ved to ganger (innboksen avviser
identiske bytes), og den som ikke har `BILAG_LES` i selskapet, ser
filnavnet men kan ikke åpne filen. Generell fildeling er en egen sak.

## 5. Levering: Postgres er sporet, NATS ringer på døren

```
skriver ──► Postgres ──► NATS REGNMED_CHAT ──► hver API-replika ──► SSE ──► nettleser
                         {samtale, melding,       (én konsument,       «se nå»   GET /chat/…
                          personene i den}         fordeler i minnet)
```

- **Hendelsen bærer bare id-er**, aldri tekst eller navn. Nettleseren
  henter selve meldingene gjennom de vaktede endepunktene, så det finnes
  ett sted som avgjør hvem som får lese.
- **En tapt hendelse koster forsinkelse, aldri en melding.** Portalen
  henter også hvert 30. sekund og når fanen blir synlig igjen.
- Strømmen er JetStream i minnet, fem minutters levetid og maks 1 MiB —
  en buss, ikke et arkiv. Hver API-replika har ÉN ordnet konsument fra
  «nå» og vekker bare sine åpne strømmer for personene hendelsen nevner.
- Agentworkeren kunngjør sine egne svar på samme strøm.
- Uten `NATS_URL` (én prosess i utvikling) går kunngjøringen rett i
  prosessens egen fordeler.
- Nettleseren snakker **aldri** NATS direkte: det ville krevd en
  NATS-identitet per bruker og flyttet tilgangsavgjørelsen ut av
  regnmed. `GET /chat/stream` er server-sent events over den vanlige
  innloggingen; portalen leser den med `fetch` fordi `EventSource` ikke
  kan sende `Authorization`.
- Strømmen sender en første hendelse straks og `X-Accel-Buffering: no`:
  nginx foran prod (og Vite i utvikling) holder ellers svaret tilbake til
  første byte. `text/event-stream` er unntatt gzip (`is_compressible`).

`since` besvares med fem sekunders overlapp: `created_at` er
transaksjonens start, så en melding kan bli synlig etter en med senere
tidsstempel. Klienten fjerner duplikater på id.

## 6. Endepunkter

| Endepunkt | Hva |
| --- | --- |
| `GET /chat/contacts` | hvem jeg kan skrive til, med hvordan (`links`: `colleague`, `agent`, `accountant`, `firm_colleague`, `client`) |
| `GET /chat/conversations` | mine samtaler, nyeste melding først, med uleste |
| `POST /chat/conversations` | åpne `{person_id, company_id?}` — `company_id` for en agent |
| `GET /chat/conversations/{id}?since=` | meldingene, motparten og `attach_to` |
| `POST /chat/conversations/{id}/messages` | `{text, documents}` — til en agent blir det en oppgave |
| `POST /chat/conversations/{id}/read` | `{until}` — merket flyttes bare framover |
| `GET /chat/unread` | `{unread, conversations}` til knappen i navbaren |
| `GET /chat/stream` | SSE, hendelse `message` uten innhold |
| `GET /firms/{fid}/contacts` | byråets valgte kontakter per oppdrag (byråmedlem) |
| `PUT /firms/{fid}/engagements/{eid}/contacts` | `{person_ids}` — byråets admin; tom liste = administratorene |

## 7. Portalen

- **Knappen i hver navbar** (`ChatButton`, inne i brukermenyen) med antall
  uleste — fra selskapene, byråvisningen, plattformen og selskapslisten.
  Meldinger står også i selskapsmenyen.
- `#/meldinger` er listen, `#/meldinger/ny` velger mottaker,
  `#/meldinger/{id}` er en samtale. Inne i et selskap åpner den samme
  meldingsappen i selskapets ramme (`#/c/{id}/meldinger/…`), så menyen
  blir stående; knappen i navbaren og menypunktet leder dit. Samtalene
  er de samme — de tilhører personen, ikke selskapet. På bred skjerm
  står liste og samtale side om side; **på telefon er det én av dem om gangen**, og adressen
  avgjør hvilken, så telefonens tilbake-knapp virker som ventet.
- Enter sender, Shift+Enter er ny linje; et bilde kan limes rett inn.
  Søkefeltet får fokus når man velger mottaker, og meldingsfeltet når en
  samtale åpnes — men ikke på berøringsskjerm, der fokus betyr et
  tastatur over halve samtalen.

## 8. Ikke bygget ennå

| Hva | Sak |
| --- | --- |
| Gruppesamtaler og rom | #250 |
| Møter med Glugg fra en samtale (nøkkelen i lenken er det vanskelige) | #251 |
| Byrået: kø over alle klienter, internt byrårom | #252 |
| Varsel om nye meldinger på e-post, senere push | #253 |
| MCP-verktøy så agenter kan lese og skrive meldinger selv | #254 |
| Tråder forankret i bilag, faktura eller utlegg | #96 |

## 9. Tester

`tests/grupper/meldinger.rs` (gruppen `tilgang_og_marked`): hvem som når
hvem fra begge sider, en fremmed agent som ikke synes, byråets valgte
kontakt (bare admin velger, bare egne medlemmer), kvittering til agenten
→ oppgave → spørsmål tilbake → svar som fortsetter samme oppgave,
sortering etter nyeste melding, uleste, utenforstående = 404, og
samtalen som forsvinner når oppdraget avsluttes.
`regnmed-mail::chatbus` har en rundtur mot en ekte nats-server.
