# Tilgang: hvem kan gjøre hva

Dette dokumentet er svaret på det spørsmålet en revisor faktisk stiller:
**hvem kan gjøre hva, og hvor er det håndhevet?**

To bevisst atskilte ting:

- **Identitet** (hvem du er): bevist av et OIDC-token fra
  påloggingstjenesten.
- **Tilgang** (hva du får gjøre, i hvilket selskap): avgjort av regnmeds
  egen database. Aldri båret i tokenet — en regnskapsfører med 60
  klienter kan ikke meningsfullt bære det i en JWT, og en
  tilgangsendring må virke uten ny innlogging.

## 1. Identitet: bare OIDC relying party

Påloggingstjenesten er **regnid** (søsterrepo). regnmed validerer
RS256-tokens mot en konfigurert issuer/JWKS
(`crates/regnmed-api/src/auth.rs`: `Verifier` + `AuthPerson`-ekstraktoren
— å legge `AuthPerson` til som handler-argument beskytter ruten). regnmed
baker aldri inn detaljer om én bestemt IdP; enhver spec-følgende issuer
virker. Konfig: `OIDC_ISSUER`, valgfri `OIDC_AUDIENCE`, `OIDC_JWKS_FILE`
(utvikling/test: statisk JWKS, signaturen valideres like fullt).

Avvist med 401, dekket av tester: manglende og oppdiktede tokens, tokens
signert med feil nøkkel, utløpte tokens, feil audience.

## 2. Aktørene

En **person** opprettes just-in-time ved første innlogging, nøklet på
tokenets `sub`. En **integrasjon** (#45, docs/integrations.md) er en
person med `kind='integrasjon'` — nettopp for at tilgangsoppslag,
attribusjon og revisjonsspor skal være identiske for robot og menneske.

Tre veier inn til et selskap:

```
person ──── company_member ─────────────────────────► selskap   («direkte»)
   │
   ├─────── firm_member ──► byrå ── oppdrag ────────► selskap   (byrånavnet)
   │
   └─────── integration_grant ──────────────────────► selskap   («integrasjon»)
```

- Et **oppdrag** er den kontraktsfestede relasjonen mellom et
  regnskaps-/revisjonsbyrå og en klient, med omfang og gyldighet. Et
  `regnskap`-oppdrag gir rollen `bokforing`; et `revisjon`-oppdrag gir
  `revisor`.
- `/me` løser token → person → alle selskapene personen kan handle for,
  hver med rollen, veien den kom gjennom og de OPPLØSTE rettighetene
  (`rettigheter`, sluggene fra vokabularet under, implikasjoner
  inkludert). Rettighetslisten er BARE visning: portalen slutter å tilby
  knapper som ville fått 403, mens vakten på serveren fortsatt avgjør
  hvert kall.
- Dette speiler Altinns delegeringsmodell (docs/gov.md), slik at
  offentlig delegering og regnmeds oppdrag kan holdes på linje.

**Rettighetene unioneres over alle veier inn.** En person kan være
direkte medlem *og* komme inn via et oppdrag. Så lenge rollene var en
stige holdt det å velge den sterkeste; etter `ansatt` (#54) er de ikke
det, og en ansatt som også kom inn gjennom et byrå ville mistet retten
til å føre sine egne timer hvis vi valgte én. `company_access` finnes
fortsatt, men bare til visning — tilgang avgjøres av `company_roles` og
unionen.

### Hvordan tilgang oppstår og opphører

| Hendelse | Virkning |
| --- | --- |
| Invitasjon løses inn ved innlogging | medlemskap opprettes (se §7) |
| Rolle endres | virker ved neste forespørsel |
| Medlemskap deaktiveres | virker straks; raden slettes aldri |
| Oppdrag avsluttes | `valid_to` er **eksklusiv** — tilgangen faller bort samme dag |
| Integrasjonsgrant tilbakekalles | samme eksklusive `valid_to`, virker straks |
| Egendefinert rolle deaktiveres | de som har den mister rettighetene straks |

## 3. Vakten

Hvert selskapsavgrenset endepunkt går gjennom **én** vakt,
`regnmed_api::tilgang::krev` — det eneste stedet i API-et som slår opp
tilgang. Endepunktet sier hvilken **rettighet** handlingen krever, ikke
hvem som får gjøre den:

```rust
krev(&state, person.person_id, company_id, Rett::FakturaSkriv).await?;
```

`krev` returnerer personens samlede tilgang, så en handler som trenger
mer enn ja/nei slipper et nytt oppslag — periodelåsen bruker det: å låse
krever `PERIODE_LAAS`, å **åpne igjen** krever admin.

**Ingen tilgang gir 404, ikke 403.** En som ikke har tilgang skal ikke
lære at selskapet finnes. `403` betyr «du har tilgang, men ikke nok».
Samme regel gjelder innenfor et selskap der omfanget er «egne data»:
kollegaens lønnsslipp og kvittering svarer 404.

**Hvorfor én vakt.** Fram til #56 hadde hver modul sin egen
`require_access` — 22 kopier i tre ulike former (`write: bool`,
`admin: bool`, et nivå som streng, og noen uten parameter i det hele
tatt). Så lenge regelen var «les eller skriv» gikk det bra, men hver
kopi var et sted å ta feil, og en fjerde rolle måtte skrives inn 22
ganger. Samme begrunnelse som ratebegrensningen for integrasjoner: én
søm ingen kan glemme.

## 4. Rettigheter og roller

**En rolle er et sett rettigheter, ikke et trinn på en stige.** Fram til
#59 var tilgang tre nivåer (`admin` > `bokforing` > `les`), lett å
forstå og umulig å bøye: enten så du hele hovedboken, eller ingenting.

Vokabularet er en **enum i koden** (`regnmed_api::tilgang::Rett`), ikke
fritekst i databasen: et endepunkt kan ikke kreve en rettighet som ikke
finnes, og kompilatoren finner alle stedene når en rettighet endrer
navn. Navnene er norske, som resten av domenet — `FAKTURA_LES`, ikke
`INVOICE_READ`.

**Rettigheter er additive, aldri subtraktive.** Det finnes ingen «alt
unntatt X»; den regelen er umulig å resonnere om når roller settes
sammen.

En **ukjent rolleverdi gir ingen rettigheter.** Før `ansatt` var `les`
svakest, så «ukjent → les» var trygt. Nå er `ansatt` og `les` ikke
sammenlignbare, og under en rullerende utrulling kan en gammel binær
møte `ansatt` i databasen — å tolke den som `les` ville vært en
**oppgradering**, ikke en degradering. Den samme fail-closed-egenskapen
er grunnen til at check-constraint-en på `company_member.role` kunne
fjernes da egendefinerte roller kom (#60): en ugyldig verdi stenger ute
i stedet for å slippe inn.

### Omfang: egne data mot alles

Noen rettigheter finnes i par, `_EGNE`/`_ALLE`: `TIMER_LES_*`,
`TIMER_SKRIV_*`, `UTLEGG_LES_*` og `LONNSSLIPP_LES_*`.

- **`_ALLE` medfører `_EGNE`.** Ellers måtte hver bunt huske begge, og
  en bunt som glemte `_EGNE` ville stengt folk ute fra deres egne data.
- Et endepunkt som allerede filtrerer på personen krever `_EGNE`; et som
  viser eller endrer andres krever `_ALLE`.

Dimensjonen ble bestemt i #59, ikke utsatt, nettopp fordi den er dyr i
ettertid: hver «egen»-variant ville blitt et nytt navn, og lagrede roller
måtte migreres.

### De innebygde rollene

| Rolle | Kort |
| --- | --- |
| `ansatt` | Selvbetjening: egne timer, eget utlegg, egen lønnsslipp, kvitteringsfoto |
| `les` | Lesing av regnskapet — **ikke lønn** |
| `revisor` | `les` + lønnsopplysningene revisjonen krever. Følger av et revisjonsoppdrag; kan ikke tildeles direkte |
| `bokforing` | `les` + alt som endrer hovedboken, + lønn |
| `admin` | `bokforing` + å styre selskapet og hvem som slipper inn |

Hver innebygd rolle bærer også en **forklaring i klartekst**
(`Rolle::beskrivelse()`, #79) som serveres på `/roles` — én kopi, rett
ved buntene som gjør den sann, vist både i rollekortet og i
invitasjonsveiledningen. En test krever at ingen innebygd rolle står
uten forklaring.

`les` ⊂ `revisor` ⊂ `bokforing` ⊂ `admin` er en egenskap ved **disse
fire**, ikke ved modellen. `ansatt` er ikke nøstet i det hele tatt, og en
egendefinert rolle trenger ikke være det.

**Ansattrollen er ikke «lesing minus noe».** Den er den formen
rangstigen ikke kunne uttrykke: en ansatt får **skrive** noen få ting —
sine egne timer, sitt eget refusjonskrav, et bilde av en kvittering — og
**lese nesten ingenting**. Før #54 måtte man gi bort bokføringstilgang
til hele hovedboken for å la noen føre timer. Bunten er positivt
avgrenset: den lister hva en ansatt får, ikke hva hun er nektet, så
resten havner ikke innenfor ved et uhell.

**Lønn er ikke allmenn lesning.** Fram til #55 lå `LONNSSLIPP_LES_ALLE`
og `LONN_LES` i lesebunten, så enhver med lesetilgang kunne laste ned
hvem som helst sin lønnsslipp — bruttolønn, forskuddstrekk, feriepenger,
fødselsdato — og se ansattlisten med månedslønn og trekkprosent. Det var
ingen beslutning; lønn kom sist og gjenbrukte den generelle
lesetilgangen.

Rettingen tvang fram et skille som uansett var riktig: **`revisor` og en
intern leser er ikke det samme.** Begge er skrivebeskyttet, men bare den
ene har en revisjonsplikt som krever lønnsopplysningene — lønn er en
vesentlig kostnad, og forskuddstrekk og arbeidsgiveravgift er lovpålagte
størrelser som skal kunne kontrolleres. Svaret på «hvor mye ser en
revisor?» er altså fortsatt «lønn også», men det er nå et **uttrykkelig
ja** i stedet for en bieffekt.

**Ingen egen lønnsrolle.** Vurdert og valgt bort: den som fører
regnskapet må uansett se lønn for å bokføre den. Et selskap som vil ha
«controller uten lønn» lager det som en egendefinert rolle (§6).

## 5. Matrisen

Hele vokabularet, og hva hver innebygd rolle gir. **Tabellen er
maskinsjekket** — `crates/regnmed-api/tests/grupper/matrise.rs` genererer den fra
`Rolle` og `Rett` og feiler hvis dokumentet ikke stemmer. En
tilgangstabell som er blitt feil er verre enn ingen tabell: den blir
sitert i en revisjon, og ingen leser koden for å kontrollere den.

<!-- MATRISE: generert av crates/regnmed-api/tests/grupper/matrise.rs -->

| Rettighet | Hva den gir | ansatt | les | revisor | bokforing | admin |
| --- | --- | --- | --- | --- | --- | --- |
| **Bilag** | | | | | | |
| `BILAG_LES` | Se bilag og vedlegg | — | ✅ | ✅ | ✅ | ✅ |
| `VEDLEGG_SKRIV` | Legge vedlegg på et bilag | — | — | — | ✅ | ✅ |
| `BILAG_LAST_OPP` | Sende dokument til innboksen | ✅ | — | — | ✅ | ✅ |
| `BILAG_BOKFOR` | Bokføre fra innboksen | — | — | — | ✅ | ✅ |
| `PERIODE_LAAS` | Låse en periode | — | — | — | ✅ | ✅ |
| `EPOST_INN_LES` | Se e-post inn til innboksen | — | ✅ | ✅ | ✅ | ✅ |
| `EPOST_INN_ADMIN` | Styre mottaksadresse og avsenderliste | — | — | — | — | ✅ |
| **Rapporter** | | | | | | |
| `RAPPORT_LES` | Se regnskapsrapportene | — | ✅ | ✅ | ✅ | ✅ |
| `MVA_ORDNING_ADMIN` | Endre mva-terminordning | — | — | — | — | ✅ |
| `BUDSJETT_LES` | Se budsjett og avviksrapport | — | ✅ | ✅ | ✅ | ✅ |
| `BUDSJETT_SKRIV` | Lage og fastsette budsjett | — | — | — | ✅ | ✅ |
| `FORANKRING_LES` | Se forankringen av hovedboken | — | ✅ | ✅ | ✅ | ✅ |
| **Faktura** | | | | | | |
| `FAKTURA_LES` | Se fakturaer | — | ✅ | ✅ | ✅ | ✅ |
| `FAKTURA_SKRIV` | Utstede faktura og kreditnota | — | — | — | ✅ | ✅ |
| `FAKTURA_SEND` | Sende faktura på e-post | — | — | — | ✅ | ✅ |
| `FAKTURAMAL_LES` | Se repeterende fakturaer | — | ✅ | ✅ | ✅ | ✅ |
| `FAKTURAMAL_SKRIV` | Endre repeterende fakturaer | — | — | — | ✅ | ✅ |
| `TILBUD_LES` | Se tilbud og ordre | — | ✅ | ✅ | ✅ | ✅ |
| `TILBUD_SKRIV` | Lage tilbud og ordre | — | — | — | ✅ | ✅ |
| `PURRING_LES` | Se purringer og forfalte krav | — | ✅ | ✅ | ✅ | ✅ |
| `PURRING_SKRIV` | Sende purring og inkassovarsel | — | — | — | ✅ | ✅ |
| **Reskontro** | | | | | | |
| `RESKONTRO_LES` | Se kunder, leverandører og åpne poster | — | ✅ | ✅ | ✅ | ✅ |
| `RESKONTRO_SKRIV` | Endre kontakter og matche åpne poster | — | — | — | ✅ | ✅ |
| `KONTAKT_SKRIV` | Endre kontaktinfo på en part | — | — | — | ✅ | ✅ |
| **Bank** | | | | | | |
| `BANK_LES` | Se bankavstemming | — | ✅ | ✅ | ✅ | ✅ |
| `BANK_AVSTEM` | Importere kontoutdrag og matche | — | — | — | ✅ | ✅ |
| `OCR_LES` | Se OCR-innbetalinger | — | ✅ | ✅ | ✅ | ✅ |
| `OCR_IMPORT` | Importere OCR-fil | — | — | — | ✅ | ✅ |
| `VALUTA_LES` | Se valutakurser | — | ✅ | ✅ | ✅ | ✅ |
| `VALUTA_SKRIV` | Legge inn og hente valutakurser | — | — | — | ✅ | ✅ |
| **Betaling** | | | | | | |
| `BETALING_LES` | Se betalingslister | — | ✅ | ✅ | ✅ | ✅ |
| `BETALING_OPPRETT` | Opprette betalingsliste | — | — | — | ✅ | ✅ |
| `BETALING_GODKJENN` | Godkjenne betalingsliste | — | — | — | ✅ | ✅ |
| `BETALING_OPPGJOR` | Registrere at betalingene er utført | — | — | — | ✅ | ✅ |
| **Produkter** | | | | | | |
| `PRODUKT_LES` | Se produktregisteret | — | ✅ | ✅ | ✅ | ✅ |
| `PRODUKT_SKRIV` | Endre produktregisteret | — | — | — | ✅ | ✅ |
| `LAGER_LES` | Se lagerbeholdning | — | ✅ | ✅ | ✅ | ✅ |
| `LAGER_SKRIV` | Registrere lagerbevegelser og varetelling | — | — | — | ✅ | ✅ |
| **Anlegg** | | | | | | |
| `ANLEGG_LES` | Se anleggsregisteret | — | ✅ | ✅ | ✅ | ✅ |
| `ANLEGG_SKRIV` | Registrere, avskrive og avhende anleggsmidler | — | — | — | ✅ | ✅ |
| **Timer** | | | | | | |
| `TIMER_LES_EGNE` | Se sine egne timer | ✅ | ✅ | ✅ | ✅ | ✅ |
| `TIMER_LES_ALLE` | Se alles timer | — | — | ✅ | ✅ | ✅ |
| `TIMER_RAPPORT_LES` | Se timeoversikt per prosjekt og ufakturert | — | ✅ | ✅ | ✅ | ✅ |
| `TIMER_SKRIV_EGNE` | Føre sine egne timer | ✅ | — | — | ✅ | ✅ |
| `TIMER_SKRIV_ALLE` | Rette alles timer | — | — | — | — | ✅ |
| `TIMER_FAKTURER` | Fakturere førte timer | — | — | — | ✅ | ✅ |
| `TIMER_SATS_SKRIV` | Sette timesatser på prosjekter og overstyre sats på timeføringer | — | — | — | ✅ | ✅ |
| `TIMER_LAAS` | Låse timelisten for en måned | — | — | — | — | ✅ |
| **Utlegg** | | | | | | |
| `UTLEGG_LES_EGNE` | Se sine egne utlegg | ✅ | ✅ | ✅ | ✅ | ✅ |
| `UTLEGG_LES_ALLE` | Se alles utlegg | — | ✅ | ✅ | ✅ | ✅ |
| `UTLEGG_SKRIV_EGNE` | Sende inn eget utlegg og kjøregodtgjørelse | ✅ | — | — | ✅ | ✅ |
| `UTLEGG_GODKJENN` | Godkjenne og avvise utlegg | — | — | — | ✅ | ✅ |
| `UTLEGG_UTBETAL` | Registrere utbetaling av utlegg | — | — | — | ✅ | ✅ |
| **Lønn** | | | | | | |
| `LONN_LES` | Se ansattregisteret og lønnskjøringene | — | — | ✅ | ✅ | ✅ |
| `LONNSSLIPP_LES_EGEN` | Se sin egen lønnsslipp | ✅ | — | ✅ | ✅ | ✅ |
| `LONNSSLIPP_LES_ALLE` | Se alles lønnsslipper | — | — | ✅ | ✅ | ✅ |
| `LONN_SKRIV` | Registrere ansatte | — | — | — | ✅ | ✅ |
| `LONN_KJOR` | Kjøre lønn | — | — | — | ✅ | ✅ |
| **Dimensjoner** | | | | | | |
| `DIMENSJON_LES` | Se avdelinger og prosjekter | ✅ | ✅ | ✅ | ✅ | ✅ |
| `DIMENSJON_SKRIV` | Endre avdelinger og prosjekter | — | — | — | ✅ | ✅ |
| **Aksjonærer** | | | | | | |
| `AKSJEBOK_LES` | Se aksjeeierboken | — | ✅ | ✅ | ✅ | ✅ |
| `AKSJEBOK_SKRIV` | Registrere aksjonærer, hendelser og utbytte | — | — | — | ✅ | ✅ |
| **Attestering** | | | | | | |
| `ATTESTERING_LES` | Se attesteringssporet | — | ✅ | ✅ | ✅ | ✅ |
| `ATTESTERING_UTFOR` | Attestere bilag | — | — | — | ✅ | ✅ |
| `ATTESTERING_ADMIN` | Sette attesteringspolicyen | — | — | — | — | ✅ |
| **Selskap** | | | | | | |
| `SELSKAP_LES` | Se firmaopplysningene | — | ✅ | ✅ | ✅ | ✅ |
| `SELSKAP_ADMIN` | Endre firmaopplysningene | — | — | — | — | ✅ |
| `MEDLEM_ADMIN` | Gi og fjerne tilgang | — | — | — | — | ✅ |
| `OPPDRAG_LES` | Se oppdrag | — | ✅ | ✅ | ✅ | ✅ |
| `OPPDRAG_ADMIN` | Inngå og avslutte oppdrag | — | — | — | — | ✅ |
| `INTEGRASJON_LES` | Se integrasjoner | — | ✅ | ✅ | ✅ | ✅ |
| `INTEGRASJON_ADMIN` | Slippe til en integrasjon | — | — | — | — | ✅ |
| `MIGRERING_ADMIN` | Importere regnskap fra et annet system | — | — | — | — | ✅ |

<!-- /MATRISE -->

Egendefinerte roller står ikke i tabellen — de er per selskap. De består
av de samme rettighetene, med unntakene i §6.

## 6. Egendefinerte roller

Et selskap setter sammen sine egne roller av rettighetene som finnes:
«Fakturaansvarlig», «Controller uten lønn», «Lagermedarbeider».
`company_role` + `company_role_right`, per selskap, og
`company_member.role` peker på navnet.

**De innebygde rollene ligger IKKE i databasen.** De er definert i
`regnmed-api::tilgang` og blir der. To definisjoner av det samme er to
steder å drive fra hverandre, og et regnskapssystem har ikke råd til at
tilgang betyr én ting i koden og en annen i basen. Tabellen holder bare
det selskapet har funnet på selv; portalen får de innebygde beskrevet
fra koden via `GET …/roles`.

**Rollenavnet er permanent**, som en dimensjonskode og av samme grunn:
det er nøkkelen medlemskapene peker på. De innebygde navnene er
reservert, ellers ville en selskapsdefinert «admin» skygget for den
ekte.

**Rettigheter som styrer hvem som har tilgang kan ikke delegeres.**
`MEDLEM_ADMIN`, `SELSKAP_ADMIN`, `OPPDRAG_ADMIN` og `INTEGRASJON_ADMIN`
er utenfor rekkevidde for en egendefinert rolle
([`Rett::kan_delegeres`]) — en rolle som kan endre tilganger kan gi seg
selv alt annet, og da er resten av avgrensningen bare pynt. Avvist når
rollen lages, ikke bare ignorert ved oppslag.

**Ukjente rettighetsnavn oppfører seg forskjellig i de to retningene**,
og det er med vilje: når et *menneske* skriver en rolle, avvises et
ukjent navn høylytt (en rolle som stilltiende mangler halve innholdet er
verre enn en feilmelding); ved *oppslag* ignoreres det (der er det en
gammel database eller en tilbakerullet versjon, ikke en skrivefeil).

En rolle **slettes aldri**, den deaktiveres — og en deaktivert rolle gir
ingenting. Endringene ligger i `company_role_change`.

**Hver endring er én transaksjon** (#62). Rollen, rettighetene og
loggraden skrives sammen eller ikke i det hele tatt: en rolle som finnes
uten at `company_role_change` forklarer hvordan, er nøyaktig det
endringsloggen finnes for å umuliggjøre. Og fordi det å sette
rettigheter er `delete` + `insert`, ville et samtidig oppslag fra vakten
utenfor en transaksjon kunne lest *mellom* dem og sett en tom liste — den
som har rollen ville mistet tilgangen et øyeblikk, tilfeldig, i en helt
annen forespørsel. Nå ser oppslaget alltid enten den gamle eller den nye
listen. Rollen låses (`for update`) før listen skrives om, ellers ville
to samtidige endringer begge slettet den gamle og sluppet igjennom hver
sin — altså unionen, som er mer tilgang enn noen av dem ba om. Testene
står i `tests/grupper/tilgang.rs` og er sett feile uten rettingen.

Integrasjoner går gjennom samme oppslag, så en maskin kan få en
egendefinert rolle og dermed nøyaktig `FAKTURA_LES` og ingenting mer. At
`admin` ikke er grantbart til en maskin står fortsatt.

## 7. Medlemsadministrasjon

Fram til migrasjon 0037 kunne **ingen gi noen tilgang**. De to veiene inn
var å opprette selskapet selv (og bli admin) eller å få et oppdrag fra
et byrå — et selskap kunne altså ikke ta inn sin egen interne
regnskapsfører eller en ansatt.

Nå: `MEDLEM_ADMIN` gir rett til å invitere, endre rolle og fjerne
tilgang, under `/companies/{id}/access…` og `/companies/{id}/invitations…`.

`person` opprettes just-in-time fra tokenets `sub`, så en som aldri har
brukt regnmed **finnes ikke å slå opp**. En invitasjon peker derfor på en
e-postadresse og blir til et medlemskap når adressen logger inn. Det
løses inn i `/me`, altså når portalen starter en økt — samme mønster som
oppdrag, der tilgangen blir synlig uten ny innlogging. Å legge oppslaget
i `AuthPerson`-ekstraktoren ville kostet en spørring på *hver*
forespørsel for noe som skjer én gang.

**Adressen må være den IdP-en oppgir.** `ensure_person` skriver
`person.email` fra hver innlogging, så en invitasjon til en privat
adresse blir aldri løst inn om tokenet bærer jobbadressen.
Normaliseringen (trimming og små bokstaver) skjer ett sted,
`medlemmer::normaliser_epost`, og det er den normaliserte formen som
lagres.

**Svaret røper ikke om adressen alt har en bruker hos oss.** Et oppslag
«finnes denne e-posten» ville gjort enhver selskapsadmin i stand til å
kartlegge hvem som er bruker på plattformen, ett forsøk om gangen. Både
det kjente og det ukjente tilfellet gir samme svar, og det har sin egen
test.

**Invitasjonen sendes som e-post** (#66), på den samme mail-skinnen og i
den samme insert-only `utsendelse`-loggen som faktura og purring —
migrasjon 0044 utvider bare hva en utsendelsesrad kan peke på. Adressen
får vite hvem som ga tilgang, til hvilket selskap, med hvilken rolle, og
en lenke til portalen.

**Det ligger ingen hemmelighet i e-posten.** Lenken går til portalens
forside og ikke noe mer; innløsningen er fortsatt at adressen logger inn
gjennom IdP-en og `/me` finner invitasjonen. En videresendt
invitasjons-e-post gir altså mottakeren ingenting — som er nettopp
grunnen til at det ikke er noe i den verdt å stjele. Lenkeadressen er
konfigurasjon (`PORTAL_BASE_URL`), aldri forespørselens `Host`-header:
en e-post vi sender skal ikke kunne pekes noe sted av den som kalte oss.

**E-posten kan aldri velte invitasjonen.** Invitasjonen ER tildelingen;
e-posten er bare varselet om den. Er køen nede, opprettes invitasjonen
likevel, og svaret sier `epost_sendt: false` med grunnen — ellers ville
et driftsavbrudd på NATS tatt med seg medlemsadministrasjonen. Portalen
sier fra om det samme, så en admin vet at hun må si fra i en annen
kanal. `POST …/invitations/{id}/resend` sender på nytt (egen
utsendelsesrad, invitasjonen selv røres ikke), og listen over åpne
invitasjoner bærer `sist_sendt`.

### Det som ikke kan skje

- **Et selskap kan ikke bli stående uten administrator.** Den siste kan
  verken degradere eller fjerne seg selv. Kontrollen kjører inne i
  transaksjonen, etter endringen, med `for update` på selskapets
  medlemsrader først — uten låsen kunne to samtidige degraderinger begge
  se «det finnes en annen admin».
- **Tilgang gjennom et oppdrag kan ikke endres her.** Den følger
  engasjementet. Listen viser vedkommende, merket `kan_endres: false`, og
  et forsøk avvises med en forklaring i stedet for å se ut som om det
  virket.
- **Medlemskap slettes aldri**, det deaktiveres — det er historikken over
  hvem som hadde tilgang.

En invitasjon kan peke på en **egendefinert rolle som blir deaktivert før
den løses inn**. Medlemskapet opprettes da med en rolle som ikke gir
noe — vedkommende kommer inn i selskapet og får ingenting. Det er valgt,
ikke oppdaget: alternativet ville vært å avvise innløsningen, og da måtte
den inviterte fått vite hvorfor, altså at selskapet har en deaktivert
rolle med det navnet. Fail-closed er riktigere enn å lekke, og en admin
som ser medlemmet uten tilgang kan gi det en annen rolle med én gang.

Hver endring havner i `company_member_change`, som er innsettings-bar:
hvem fikk hva, når, og hvem som ga det. En innløst invitasjon har ingen
utfører (personen løste den inn selv); hvem som inviterte står på
invitasjonen.

## 8. Grensen mot plattformen: hva som finnes, og hva som aldri gjør det

### Ingen vei inn i hovedboken krysser selskapsgrenser

**Ingen tilgangsvei inn i et selskaps regnskap krysser selskapsgrenser.**
Alle tre veiene i §2 er avgrenset til ett selskap i selve datamodellen —
`company_member`, `engagement` og `integration_grant` bærer hver sin
`company_id`, og tilgangsoppslaget (`company_access_for_person` i
tenancy.rs) kjenner ingen jokertegn. Plattformrollene i neste avsnitt
går ikke gjennom det oppslaget og gir **ingen** selskabstilgang: bilag,
saldoer, rapporter og reskontroposter er utenfor rekkevidde for enhver
leverandørrolle.

Det ble besluttet i #57, og begrunnelsen er produktets egen
tillitshistorie: hovedboken selges som etterprøvbar («ikke stol på oss —
kontrollér»), og en global administrator er nøyaktig den bakveien vi
selger fraværet av. En revisor som spør «hvem hos leverandøren kan lese
klientens regnskap?» skal få svaret *ingen* — og takket være
forankringen (docs/anchoring.md) er også den påstanden kontrollerbar i
den delen som betyr mest: en omskrevet hovedbok kan bevises, uansett
hvem som skrev.

Avgjørelsen er festet i test, ikke bare i tekst:
`an_admin_crosses_no_company_boundary` i `tests/grupper/tilgang.rs` viser at den
sterkeste selskapsrollen som finnes er en fullstendig fremmed i
naboselskapet — 404 på lesing, skriving og administrasjon, og `/me`
nevner ikke selskapet. Og
`a_company_admin_is_a_stranger_to_the_platform_and_vice_versa` i
`tests/grupper/plattform.rs` fester den andre retningen: en
plattform-systemadmin får 404 på selskapets hovedbok, stamdata og
administrasjon. Skal noen av grensene flyttes, må testene endres
bevisst, og dette avsnittet med dem.

### Plattformrollene systemadmin og support — den avgrensede unntaksveien

Bygget 2026-08-03, etter kravlisten #57 selv stilte opp. En
plattformrolle når **administrative stamdata på tvers av selskaper og
byråer** — personer, medlemskap, invitasjoner-i-praksis (tildeling av
medlemskap) og kunderegistre — og ingenting i noen hovedbok. Alt den
kan ligger under `/platform/*` på sin egen sub-router
(`regnmed-api::plattform`); tilgangsvakten `tilgang::krev` og
tenancy-oppslaget er uendret.

To roller, lagret i `platform_member` (migrasjon 0049):

- **`support`** ser selskaper, byråer og brukere med tilknytninger, og
  kan tildele et **nytt** medlemskap (innebygde roller). Den ser ikke
  kunderegistre og administrerer ikke plattformbrukere.
- **`systemadmin`** ser i tillegg kunderegistrene (stamdata med eierens
  selskap navngitt — aldri saldoer), endrer eksisterende medlemskap og
  roller, og administrerer plattformmedlemskapene selv.

Kravene fra #57 er håndhevet strukturelt, ikke i rutiner:

1. **Logget.** `vakt`-middlewaren omslutter hele sub-routeren: token →
   person → aktiv rolle → loggrad i `platform_access_log`
   (innsettings-bar) → handler. Raden skrives FØR handleren avgjør noe
   (et avvist forsøk er også synlig) og synkront — feiler
   logginnsettet, feiler kallet. Et /platform-endepunkt som glemmer
   loggen kan ikke eksistere.
2. **Synlig for den det gjelder.** `GET /companies/{id}/platform-access`
   (og byråtvillingen `GET /firms/{id}/platform-access`) serverer radene
   som angår selskapet til selskapets egne administratorer — vist i
   portalen under Brukere. Medlemskap plattformen tildeler havner i
   `company_member_change`/`firm_member_change` med **`kilde =
   'plattform'`** (0049 utvider sjekkene), i samme logg som alle andre
   tilgangsendringer.
3. **Tidsbegrenset.** `platform_member.valid_to` er NOT NULL — en
   plattformrolle uten utløpsdato kan ikke skrives inn. Datoen er
   eksklusiv og sjekkes per forespørsel; tilbakekalling
   (`DELETE /platform/members/{id}`, eller `regnmed platform-end`)
   virker på neste kall. Notat med begrunnelse er obligatorisk.

Førstegangs tildeling skjer i CLI-et (`regnmed platform-grant`) — den
første systemadminen kan ikke komme gjennom et API bare systemadminer
får kalle. Deretter administreres medlemskapene via
`/platform/members`. Roller gis bare til mennesker; en
integrasjonsidentitet avvises både ved tildeling og i vakten. En kundes
tilknytning til sitt selskap er for øvrig **fast**: part-id-en er del av
hasjkjeden, så det finnes bevisst ingen funksjon som flytter en kunde
mellom selskaper.

**Støtteveien via kunden består** og er fortsatt førstevalget der den
strekker til: en **invitasjon** (§7) med den minste rollen som holder,
eller et **oppdrag** til et byrå — synlig, logget, trekkbar samme dag.
Plattformrollen finnes for det invitasjonen ikke dekker (feiladresserte
invitasjoner, låste medlemskap, onboarding-hjelp), og prisen for den er
betalt i punktene over: den er logget, synlig og tidsbegrenset, bygget
som om misbruk antas.

**Selskapet uten administrator.** I normal drift kan det ikke oppstå —
den siste administratoren kan verken degradere eller fjerne seg selv
(§7). Men et dødsfall eller en brå avslutning spør ikke systemet først.
For det tilfellet finnes en **nødprosedyre**, og den går gjennom
databasen, ikke API-et — for det finnes ingen API-vei, og det skal det
ikke gjøre:

1. Selskapet ber om det skriftlig, fra noen med rett til å representere
   det (styreleder, daglig leder, eier). Samtykket arkiveres og får en
   referanse.
2. Den som skal overta logger inn i portalen én gang, slik at personen
   finnes (`person` opprettes just-in-time, §7).
3. En operatør med databasetilgang utfører, i én transaksjon:

   ```sql
   begin;
   insert into company_member (company_id, person_id, role)
   values ('<selskap>', '<person>', 'admin')
   on conflict (company_id, person_id)
       do update set role = 'admin', active = true;
   insert into company_member_change
       (id, company_id, person_id, endring, til_rolle, kilde, notat)
   values (gen_random_uuid(), '<selskap>', '<person>',
           'lagt_til', 'admin', 'nodprosedyre',
           '<samtykkereferanse>');
   commit;
   ```

4. Kunden bekrefter at tilgangen virker, og rydder selv videre med den.

Sporet **heter det det er**: `kilde = 'nodprosedyre'` (migrasjon 0040)
står i den samme endringsloggen som alle andre tilgangsendringer,
synlig for kundens admin og for revisoren. Uten en egen kilde måtte
innslaget ha utgitt seg for å være en vanlig admin-handling —
tilgangsloggen ville løyet om akkurat det innslaget den finnes for å
fange. Og et nødinnslag *uten* samtykkereferanse avvises av selve
databasen (check-constraint i 0040): det uattribuerte inngrepet er
nettopp det prosedyren skal umuliggjøre.

Merk hva prosedyren IKKE kan: databasetilgang gir ingen vei rundt
hovedbokens vern. Endring og sletting stoppes av triggerne, og en
omskriving på DBA-nivå bryter hasjkjeden mot den offentlig forankrede
merkleroten (docs/anchoring.md).

Med plattformrollene på plass er nødprosedyren sjeldnere nødvendig —
en systemadmin kan tildele en ny administrator gjennom
`/platform/users/{pid}/companies/{cid}`, logget og synlig som
beskrevet over. Databaseprosedyren består for tilfellet der ingen
plattformrolle finnes eller selve API-et er utilgjengelig; kravet om
skriftlig samtykke med referanse gjelder begge veier.

### Det som ellers ikke finnes

- **Ingen tilgang i tokenet.** Tokenet beviser identitet, ingenting
  annet. Derfor virker en tilbakekalling straks, uten å vente på at et
  token løper ut.
- **Ingen tilgangsstyring i portalen.** Portalen skjuler menyvalg og
  knapper for å slippe at man klikker seg inn i en feilmelding. Det er
  en bekvemmelighet. **Serveren nekter**, og det er der sannheten
  ligger.
- **Driftsoppgavene går utenom.** `regnmed anchor`,
  `generate-invoices`, `depreciate`, `migrate`, `verify-ledger` og
  `saft-export` kjører som CLI/CronJob rett mot databasen, uten noen
  person og uten denne vakten. De er maskinelle og tar ingen avgjørelse
  et menneske skulle tatt. Det er en bevisst grense (docs/deploy.md),
  og den står her — rett ved plattformavgjørelsen — for at den ikke
  skal se ut som et hull.

## 9. Grensene mot resten av systemet

Tilgang er ett lag av flere, og de gjør forskjellige ting:

| Mekanisme | Hva den stopper |
| --- | --- |
| **Tilgang** (dette dokumentet) | hvem som får utføre en handling |
| **Attestering** (docs/attestering.md) | en EKSTRA sperre oppå tilgang: at *en annen* har godkjent før bokføring eller betaling. Ikke en erstatning — den som attesterer må uansett ha tilgang |
| **Periodelås / timelås** | tidssperrer, ikke tilgangssperrer: en admin med all verdens rettigheter kan ikke bokføre i en låst periode |
| **Abonnementssperren** (docs/abonnement.md) | en betalingssperre, håndhevet i samme vakt: et sperret abonnement stopper endrende rettigheter (`Rett::endrer`), aldri lesing, eksport eller styringen av selskapet |
| **Append-only-hovedboken** (docs/ledger.md) | gjelder uansett rolle. En admin kan heller ikke endre eller slette et bilag — korreksjon er et reverserende bilag |

Det siste punktet er verdt å si tydelig: **ingen rolle i dette
dokumentet gir rett til å endre historikk.** Tilgangsmodellen avgjør hvem
som får legge noe til, aldri hvem som får ta noe bort.

### Herding av HTTP-svarene (#64)

Tilgangsvakten avgjør *om* du får bytene. Dette laget avgjør hva
nettleseren gjør med dem, og finnes fordi forsvaret ellers hviler på
vaner: portalen laster ned via `blob` + `a.download` og navigerer aldri
til et dokument, Bearer-modellen gjør at en lenke ikke kan «åpnes
autentisert», og Svelte escaper det den rendrer. Alt sammen stemmer i
dag. Ingen av delene er håndhevet.

To sømmer, av samme grunn som vakten er én søm:

| Søm | Hva den gjør |
| --- | --- |
| `herding::security_headers` (middleware på ALT) | `X-Content-Type-Options: nosniff` og `Referrer-Policy: no-referrer` på hvert svar; `Content-Security-Policy` på HTML |
| `herding::file_response` (hver eneste nedlasting) | saniterer filnavnet og bestemmer hva serveren påstår at bytene er |

**Opplasteren bestemmer ikke hva serveren sier bytene er.** En `ansatt`
eller en tillatt e-postavsender kunne ellers fått oss til å servere
`text/html` fra vårt eget opphav — og `nosniff` hjelper ikke da, for
typen ville vært vår egen påstand. Bare kjente typer serveres
(dokumenter, bildeformater, XML, tekst); alt annet blir
`application/octet-stream`, og `inline` nedgraderes til `attachment`.
**`image/svg+xml` står bevisst ikke på listen** — en SVG er et dokument
som kan kjøre skript, og er den ene bildetypen som aldri skal serveres
som bilde. Bytene selv røres aldri: dokumentet er bevis (migrasjon 0015).

Filnavnet er den andre halvparten. Et anførselstegn lukket
quoted-stringen i `Content-Disposition` og lot opplasteren legge til egne
parametere; CR/LF avsluttet headerlinjen og ga en 500. Nå saniteres
ASCII-formen, og `filename*=UTF-8''…` bærer de norske bokstavene.

CSP-en henter hashen til portalens ene innebygde skript **fra HTML-en som
faktisk serveres**. En hash festet i koden ville stilltiende sluttet å
matche den dagen noen redigerte skriptet — temablinket ville kommet
tilbake, og ingen ville koblet det til en header. `script-src` har ikke
`unsafe-inline`; det er den klausulen som gjør en glemt escape til en
konsollfeil i stedet for et innbrudd. `style-src` tillater inline stil,
fordi nøkkeltallsøylene er ren CSS (#36) og injisert stil er et
vesentlig mindre problem enn injisert skript.

## 10. I portalen

**«Inviter folkene dine»** (#79): så lenge selskapet har nøyaktig ett
direkte medlem, viser Oversikt et veiledningskort for de første
invitasjonene. Typiske profiler er snarveier til riktig rolle
(lønnsmottaker → `ansatt`, økonomiansvarlig → `bokforing`, medeier →
`admin`), med rollens forklaring fra `/roles` synlig der valget tas.
Den eksterne regnskapsføreren er med vilje **ikke** en invitasjon —
kortet peker til oppdragskatalogen, siden tilgang gjennom et oppdrag
følger avtalen og kan avsluttes samme dag. Åpne invitasjoner vises i
samme kort med sendt-status; kortet forsvinner når medlem nummer to er
inne, og Oppdrag → Tilgang tar over. Kortet er bare en visning: alle
kallene går til de samme MEDLEM_ADMIN-vaktede endepunktene.

Under **Oppdrag** ligger tre kort: Tilgang (hvem som kommer til), Roller
og Integrasjoner. Rollekortet viser de innebygde rollene som de er —
uendrelige, men leselige, så en admin ser hva de faktisk betyr uten å
måtte lese koden — og et rutenett for selskapets egne.

Rutenettet viser **hva rettigheten lar deg gjøre**, ikke slug-en: «Se
alles timer», ikke `TIMER_LES_ALLE`. Teksten og grupperingen kommer fra
`Rett::beskrivelse()` og `Rett::gruppe()` og serveres av API-et, så det
finnes bare én liste — portalen har ingen egen kopi, og en ny rettighet
kan ikke bli stående uten forklaring (egen test).

De fire rettighetene som ikke kan delegeres vises som avkryssede bokser
i grå, med begrunnelsen ved siden av: *«kan bare gis av admin, siden den
styrer hvem som har tilgang»*. Konsekvensen står der valget tas, ikke i
en hjelpetekst.

Gruppene er `<details>` og ikke en bred tabell — det er formen som tåler
375 px uten å rulle sidelengs. **Portalen skjuler, serveren nekter:**
rutenettet er en visning av det som allerede håndheves, og en meny uten
et valg er en bekvemmelighet, ikke en sperre.

## 11. Hvor det er testet

Autorisasjon testes som **nektelser**. At en admin slipper til er dekket
overalt ellers i suiten; at en leser *ikke* slipper til er det ingenting
annet som fanger.

| Fil | Hva den fester |
| --- | --- |
| `tests/grupper/me_endpoint.rs` | Identiteten: lokalt generert JWKS signerer ekte RS256-tokens, et byrå-med-oppdrag pluss et direkte medlemskap løses til nøyaktig forventet selskapsliste, og forfalskede/utløpte/feil-audience-tokens avvises |
| `tests/grupper/tilgang.rs` | Tilgangsmatrisen mot en ekte server: at `les` ikke får endre noe, at `bokforing` ikke får administrere, at en ansatt ikke kommer til hovedboken, at lønn ikke er allmenn lesning, at en egendefinert rolle gir akkurat det den sier, at en utenforstående får 404 og ikke 403 — og at admin i ett selskap er en fullstendig fremmed i et annet (§8), inkludert at nødprosedyren krever referanse og navngir seg selv i sporet |
| `tests/grupper/medlemmer.rs` | Hele livsløpet: invitasjon → innlogging → medlemskap, at svaret ikke røper om brukeren finnes, at siste admin ikke kan fjerne seg selv, at oppdragstilgang ikke kan endres herfra, og at sporet navngir hvem som ga hvem tilgang |
| `tests/grupper/plattform.rs` | Plattformgrensen (§8) begge veier: en selskapsadmin er fremmed for `/platform`, en plattform-systemadmin får 404 på hovedbok/stamdata/administrasjon i ethvert selskap; hvert kall logges (også avviste) og loggen leses av selskapets admin men ikke av `les`; support får ikke kunderegistre, medlemsendringer eller plattformadministrasjon; tilbakekalling virker på neste forespørsel; tildelinger står med `kilde='plattform'` i selskapets egen logg |
| `tests/grupper/matrise.rs` | At tabellen i §5 stemmer med koden, og at den dekker hele vokabularet |
| `regnmed_api::tilgang` sine enhetstester | At buntene ikke overlapper, at slug-ene er unike og går rundtur, at `_ALLE` medfører `_EGNE`, at hver rettighet har forklaring og gruppe, og at en ukjent rolleverdi gir **ingen** rettigheter |

### Tre feller, alle gått i under bygging

Dette avsnittet står her fordi hver av dem ga en test som så grønn ut
mens den målte ingenting.

1. **Enhetstestene kan ikke fange en rettighet i feil bunt.** De utleder
   fasiten sin *fra* buntene. Prøvd med vilje — `PRODUKT_SKRIV` flyttet
   til lesebunten — og samtlige besto. Det er `tests/grupper/tilgang.rs` som er
   sperren der, og den slo ut.
2. **Kroppen må være gyldig JSON for endepunktet.** axum kjører
   `Json<T>`-uttrekket før handleren, så en tom kropp gir 422 og vakten
   blir aldri spurt. En matrise med `{}` består uten å bevise noe.
3. **Spørrestrengen må også være gyldig.** Samme mekanisme med
   `Query<T>`: `/bank/reconciliation` uten `?account=` gir 400 før
   vakten. Felle nummer to og tre er samme feil i to former, og den
   andre ble gjort etter at den første var oppdaget.

Mønsteret er verdt å ta med videre: **en autorisasjonstest som ikke er
sett feile, er ikke verifisert.** Hver av sperrene over er kontrollert
ved å ødelegge det den skal fange.
