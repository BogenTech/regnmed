# Maskin-tilgang til API-et

Issue #45. Nettbutikken, kassasystemet og selskapets egne script skal
kunne kalle API-et uten at et menneske logger inn — uten at det åpner
en ny vei inn i bøkene som ikke er like godt bevoktet som den gamle.

Den offentlige endepunktoversikten står i [api.md](api.md).

## Modellen: samme prinsipal, samme regler

> **Tokenet beviser identitet. regnmed avgjør hva identiteten får gjøre.**

Det er nøyaktig samme setning som for mennesker (docs/auth.md), og det
er derfor maskin-tilgang ikke krevde en ny autorisasjonsvei:

- **Identiteten** kommer fra vår IdP som `client_credentials` — regnmed
  utsteder aldri egne API-nøkler. Én type legitimasjon i plattformen,
  ett sted å trekke den tilbake.
- **Tilgangen** er en rad i regnmeds base: en admin gir integrasjonen
  tilgang til sitt selskap på et nivå. Uten grant får den ingenting,
  uansett hvor gyldig tokenet er (testet).

Teknisk er en integrasjon en `person` med `kind = 'integrasjon'`. Det
høres ut som en snarvei, men er det motsatte: tilgangsoppslaget,
attribusjonen og revisjonssporet blir de samme for en robot som for et
menneske, og da finnes det ingen egen maskinvei som kan utvikle sine
egne hull.

### Avhengigheten, og hva som gjenstår

regnid støtter `client_credentials` (regnids migrasjon 0007). regnmed
virker med et hvilket som helst token fra den konfigurerte issueren der
`sub` er integrasjonens `client_id` — det er alt kontrakten krever, og
den siden har vært ferdig hele tiden.

Grantet er **av som standard per klient**: en admin må registrere
integrasjonen som konfidensiell klient i regnid og gi den grantet
uttrykkelig:

```sh
regnid add-client --client-id <robot> --name '<navn>' \
    --grant-type client_credentials --confidential --audience regnmed
```

Hemmeligheten skrives ut én gang; regnid lagrer bare hashen. En
offentlig klient får aldri et token uten bruker, uansett hva raden sier,
og en vanlig konfidensiell webklient har ikke grantet med mindre noen
har gitt det.

**Verifisert på tvers 2026-07-27:** ekte regnid → ekte regnmed-api.
`/me` svarer 200 med `sub` = klientens id og `companies: []` — altså
identiteten bevist og ingen tilgang før noen gir den, som er hele
poenget med modellen. Et ugyldig token gir 401.

Gjenstår før dette virker i produksjon: regnid må rulles ut med sin
migrasjon 0007. Klientadministrasjonen i regnids admin-UI oppretter
fortsatt bare innloggingsklienter — maskinklienter registreres med
CLI-en, bevisst en mer overveid handling.

## Tilgangen

| Nivå | Kan |
| --- | --- |
| `les` | Alt en revisor kan lese: rapporter, reskontro, bilag |
| `bokforing` | I tillegg: bokføre, fakturere, laste opp i innboksen |

`admin` er bevisst ikke mulig å gi en maskin. Å endre hvem som har
tilgang er en menneskelig beslutning.

Grantet er modellert som et oppdrag (docs/auth.md): `valid_to` er
**eksklusiv**, så en tilbakekalling virker i samme øyeblikk — ikke ved
midnatt. Raden blir stående med hvem som ga og hvem som trakk tilbake;
tilganger slettes ikke.

En klient-id som allerede tilhører et menneske med tilgang kan ikke
registreres som integrasjon — en robot skal aldri arve et menneskes
tilgang ved å bli registrert under subjectet deres.

## Attribusjon: sporet navngir roboten

Alt en integrasjon bokfører får `created_by` = integrasjonens navn.
Ikke «system», ikke en uuid: «Nettbutikken». Navnet settes ved
registrering og kan ikke overstyres av tokenet — ellers kunne en klient
døpt om seg selv i revisjonssporet.

## Ratebegrensning

Hver integrasjon har en grense per minutt (standard 120, justerbar per
integrasjon). Over grensen svarer API-et `429` med en tydelig melding.

Grensen er en token-bucket **per prosess**, med vilje: en ratebegrenser
som trenger sin egen datastore ville kostet mer enn budsjettet den
beskytter (docs/frugality.md). Med flere replikaer er den effektive
grensen per replika — det er en bevisst avveining, ikke en forglemmelse.

## Aktivitetslogg

- `integration_call` — hver **endrende** forespørsel (metode, sti,
  status). Det er disse en admin og en revisor vil se.
- `integration_usage` — en teller per integrasjon, selskap og dag som
  dekker alle kall, også lesingene. Å lagre hver lesing ville vært
  volum uten verdi.

Begge er synlige i portalen (Oppdrag → Integrasjoner) for alle med
tilgang til selskapet.

## Endpoints

- `GET  /companies/{id}/integrations` — tilganger, status, kall i dag
- `POST /companies/{id}/integrations` — `{client_id, navn, kontakt?, access}` (admin)
- `POST /companies/{id}/integrations/{iid}/revoke` (admin)
- `GET  /companies/{id}/integrations/log` — de endrende kallene

## Slik kommer en integrasjon i gang

1. Integrasjonen registreres som klient i regnid og får `client_id` +
   `client_secret` (regnids sak).
2. Selskapets admin gir tilgang i portalen: lim inn `client_id`, gi den
   et navn, velg nivå.
3. Integrasjonen henter token med `client_credentials` og kaller API-et
   med `Authorization: Bearer …` — samme endepunkter som portalen
   bruker.

## Tester

`regnmed-api/tests/grupper/integrasjon.rs` (ekte Postgres, også CI): et gyldig
maskintoken uten grant får `404` (ikke `403` — en fremmed skal ikke
lære at selskapet finnes); admin gir tilgang; roboten laster opp og
bokfører; `created_by` på bilaget er «Nettbutikken»; de endrende
kallene ligger i loggen og lesingene i telleren; en integrasjon med
bokføringstilgang kan ikke gi seg selv eller andre tilgang;
tilbakekalling virker samme dag, og historikken navngir den som trakk
den tilbake. En egen test tømmer ratebudsjettet og får `429`, mens
mennesket ved siden av merker ingenting.

`regnmed-api/src/auth.rs` har enhetstesten for token-bucketen.

## Bevisst utenfor

En marketplace/app-store for integrasjoner (grant-by-client-id først),
og webhooks — pull holder til noen viser at det ikke gjør det.
