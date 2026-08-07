# Kontantsalg

Salg som er betalt ved levering — **bokføringsforskriften §5-3**
(kontantsalg) og **§5-4** (dokumentasjon av kontantsalg/dagsoppgjør).

All salgsdokumentasjon forutsatte kreditt: `create_invoice_in` posterte
alltid til kundefordring med KID, og PDF-en skrev alltid
betalingsinformasjon. Enhver virksomhet med butikk, verksted, kafé eller
markedssalg var dermed utenfor systemet.

## 1. Vi er ikke et kassasystem

**Kassasystemlova** legger kassasystemet selv, med produkterklæring, på
leverandøren. Vi skal ikke være et kassasystem — vi skal kunne
**bokføre dagsoppgjøret fra ett**, og utstede kontantfaktura der
reglene tillater det. Ingen kassaskuff, ingen kvitteringsskriver, ingen
SAF-T Cash Register.

## 2. Kontantfaktura

`Dokumenttype::Kontantfaktura` er et ANNET DOKUMENT, ikke en faktura med
et flagg: tittelen KONTANTFAKTURA, «Betalt: <betalingsmiddel>» blant
dokumentfaktaene, og **ingen KID, ingen forfallsdato, ingen
betalingsinformasjon**. Å be noen betale det de allerede har betalt er
ikke en skjønnhetsfeil, det er et krav om betaling nummer to.

**Fordringen oppstår og gjøres opp i SAMME transaksjon.** Det ville vært
enklere å postere salget rett mot bank og hoppe over 1500 — og det er
nettopp det som ikke må skje: reskontro-doktrinen sier at en kundes
posteringer bærer en part, og en sidedør forbi den ville gjort
`reskontro_kontroll` (revisors avstemming) stille ufullstendig. Parten
står på BEGGE sider, og den åpne posten lukkes i det øyeblikket den
oppstår.

`POST /companies/{id}/invoices` med `kontant_betalingsmiddel` +
`oppgjorskonto`. Kontoen oppgis av kalleren — 1900 kontanter, 1920 bank
eller kortinnløserens oppgjørskonto. Vi gjetter aldri hvordan noen ble
betalt.

## 3. Kassaoppgjør

Dagens Z-rapport blir **ett bilag**: betalingsmidler debet, salg kreditt
netto per sats, mva kreditt som summen av delene. Salgslinjene beholder
mva-koden sin, så mva-spesifikasjonen ser dagens salg slik den ser en
fakturas; mva-linjen er ukodet, som i fakturamotoren — en kode der ville
telt samme grunnlag to ganger.

**Z-nummeret står i bilagsteksten** og knytter bilaget til
kassasystemets egen nummererte rapport. Den koblingen er poenget med
§5-4; et oppgjør uten den dokumenterer ingenting. Rapporten lastes opp
som vedlegg på oppgjørets eget bilag, i samme kall.

**En Z-rapport som ikke går opp avvises HØYLYTT.** Salg ≠
betalingsmidler er en ødelagt rapport, ikke en kassadifferanse, og å
balansere den mot differansekontoen ville skjult nøyaktig det tallet
oppgjøret finnes for å avstemme.

## 4. Kassadifferansen er sitt eget bilag

Differansen mellom **talt** kasse og **registrert** kontantsalg føres
som eget bilag — synlig, aldri utjevnet i stillhet. Et avvik er et funn
om dagen, ikke en avrunding av den. Manko krediterer kontantkontoen og
kostnadsfører differansen (7830 som standard); overskudd går motsatt vei
— det er ikke «ingen differanse».

Er kassen ikke talt, oppgis ingen opptalt beholdning, og **det bokføres
ingen differanse**: vi antar aldri at den stemte.

Alt skjer i ÉN transaksjon — oppgjøret, differansen og vedlegget. En dag
der differansebilaget feilet etter oppgjøret ville etterlatt en kasse
som ser avstemt ut og ikke er det.

## 5. Web API

| Endepunkt | Rett |
| --- | --- |
| `POST /companies/{id}/invoices` m/ `kontant_betalingsmiddel` | `FAKTURA_SKRIV` |
| `POST /companies/{id}/kassaoppgjor` | `BILAG_BOKFOR` |
| `POST /companies/{id}/kassaoppgjor/rapport?filename=` (Z-rapport i kroppen, oppgjøret i `X-Dagsoppgjor`) | `BILAG_BOKFOR` |

Portalen: **Kassaoppgjør**-kortet i Hovedbok-seksjonen, som viser salg
mot betalingsmidler mens du skriver og nekter å sende en Z-rapport som
ikke går opp.

## 6. Hvor det er testet

- `regnmed-core::kassa`: at bilaget går i null og splitter mva per sats,
  at en ubalansert Z-rapport avvises med hjemmelen i meldingen, og at
  differansebilaget bare finnes når det ER en differanse (begge veier).
- `crates/regnmed-db/tests/kontantsalg.rs`: kontantfakturaen som gjør
  opp sin egen fordring gjennom reskontroen (part på begge sider, ingen
  åpen post, KONTANTFAKTURA uten KID i den lagrede PDF-en), og et
  dagsoppgjør med 50 kr manko som blir to bilag, med Z-rapporten som
  vedlegg og kjeden verifisert.

## 7. Ikke bygget

Filimport av dagsrapport (v1 er skjemaet), SAF-T Cash Register, og
kontantsalg i valuta.
