# Årsavslutning

Resultatdisponering og skattekostnad — **rskl. §6-1, §6-2 og §3-1**,
jf. **asl. §8-1**.

Fram til nå ble et regnskapsår aldri avsluttet i regnmed. Det var et
bevisst valg så langt det rakk: `udisponert_resultat_ore` utledes av
klasse 3–8-summen ved hver lesning, balansen går i null uansett, og
rapportene er reproduserbare for evig. Men det holder bare til noen
spør om **fjorårets** egenkapital:

- Rskl. §6-2 krever at balansen skiller innskutt og opptjent
  egenkapital. En evig «udisponert resultat»-bøtte er ikke det skillet.
- Asl. §8-1 regner utbyttegrunnlaget ut fra nettopp den fordelingen —
  og utbyttevedtaket debiterte 2050 mot en saldo systemet aldri
  krediterte. **Kontoene var riktige hele tiden; det som manglet var
  motposten.** Årsavslutningen leverer den, så utbytteposteringen er
  uendret.

## 1. Kontoen er ikke vårt valg

Skatteetatens næringsspesifikasjons-kodeliste (vendored i
`saft/naeringsspesifikasjon_*.csv`, alt i bruk av SAF-T-eksporten) gir
**8800** sin EGEN grupperingskategori:

```
resultatDisponeringForSAF-T;…;8800;Disponering av årets overskudd/dekning av årets underskudd
```

Kategorien er atskilt fra alle resultatlinjene. Disponering er altså
ikke en resultatlinje, heller ikke etter etatens eget skjema.

## 2. Derfor trengs ingen ny tilstand

8800 ligger i **klasse 8**. Når avslutningen debiterer 8800 og
krediterer 2050:

- `udisponert_resultat_ore` = `-sum(klasse 3–8)` faller med nøyaktig det
  beløpet egenkapitalen vokser — udisponert blir null for det avsluttede
  året, og balansen går fortsatt i null.
- Ingen «dette året er lukket»-flagg må holdes synkronisert med
  hovedboken. **Hovedboken bærer det selv**, som alt annet her.

Det ENESTE `regnmed-core::regnskap` måtte endre er at
resultatregnskapet UTELATER 8800 (`resultat_sum`, mens `ledger_sum` som
balansen bruker beholder den). Uten det ville fjorårets
resultatregnskap vist null overskudd i det øyeblikket året ble
avsluttet. Testen er sett feile: `left: 0, right: 185000`.

## 3. Rekkefølgen — et avvik fra sakens ordlyd

Saken ba om at årsavslutning **forutsetter** at året er periodelåst. Det
er umulig slik låsen virker: bilaget dateres 31.12, altså inne i
perioden som da ville vært låst, og `forbid_locked_period_posting`
(migrasjon 0011) håndhever det i DATABASEN uavhengig av all
applikasjonskode. Å svekke den triggeren for årsavslutningens skyld
ville vært å pælme den ene garantien for å få den andre.

**Løsningen gir samme vern med omvendt rekkefølge:** avslutningen SETTER
låsen selv, i samme transaksjon som bilaget. Etterpå er året både
disponert og stengt for nye posteringer — som er det saken ville oppnå.
Låsen er insert-only med eget spor, så dette er en registrert handling.

Et år som allerede er låst nekter derfor å avsluttes, med en melding som
sier hvorfor i stedet for å la triggeren dukke opp to lag ned.

## 4. Skattekostnaden oppgis, den utledes ikke

`skattekostnad_ore` er kallerens tall. Skattemessig resultat er ikke
regnskapsmessig resultat — permanente og midlertidige forskjeller
avgjør, og de midlertidige finnes allerede for anleggsmidler i
`saldo_rapport`. Å regne 22 % av regnskapsmessig overskudd her ville
vært å dikte opp en skattemelding. **0 er et gyldig svar som må
oppgis.**

Utsatt skatt er bevisst ikke bygget (saken sier det samme).

## 5. Bilaget

| Linje | Konto | |
| --- | --- | --- |
| Skattekostnad | 8300 debet | utelates når skatten er 0 |
| Betalbar skatt | 2500 kredit | |
| Disponering | 8800 debet | overskudd; underskudd går motsatt vei |
| Opptjent egenkapital | 2050 kredit | |

Migrasjon 0060 `arsavslutning` er innsettings-bar med unik
(selskap, år): et år kan ikke disponeres to ganger, og en korreksjon er
et reverserende bilag som alt annet. Raden lagrer tallene slik de var —
samme grunn som balansedokumentasjonens saldo (0059): endres hovedboken
etterpå, skal forskjellen kunne ses.

## 6. Web API

| Endepunkt | Rett |
| --- | --- |
| `GET /companies/{id}/arsavslutning` | `RAPPORT_LES` |
| `GET …/arsavslutning/{ar}/forslag` | `RAPPORT_LES` |
| `POST /companies/{id}/arsavslutning` | `BILAG_BOKFOR` **og** `PERIODE_LAAS` |

Begge rettighetene kreves fordi låsen er halvparten av handlingen: den
som ikke får låse en periode skal ikke kunne avslutte et år heller.

Portalen: **Årsavslutning**-kortet under Administrasjon → Periode.

## 7. Ikke bygget

Utsatt skatt, oppstillingsplanen etter rskl. §6-1/§6-1a/§6-2 (vår NS
4102-gruppering er §3-1-presentasjonen) og notene — de to siste hører
til [#12](https://github.com/BogenTech/regnmed/issues/12) og er nevnt
der.
