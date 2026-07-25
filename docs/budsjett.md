# Budsjett og avviksrapport

Issue #41. Plan mot virkelighet, på tallene vi allerede stoler på:
budsjettet er det eneste tallet i systemet som er en MENING, og det
behandles deretter — mens virkeligheten hentes fra hovedboken med de
samme rene SUM-spørringene som resten av rapportene.

## Arbeidsdokument, så bevis

Et budsjett er **fritt redigerbart mens det er utkast**: linjer legges
til og fjernes, tall skrives om, hele rutenettet erstattes. Det later
ikke som det er bevis, for det er det ikke.

Når noen **fastsetter** det, fryses versjonen — raden og linjene blir
uforanderlige (trigger i migration 0031), og hvem som fastsatte og når
lagres. En revisjon er ikke en endring; det er en **ny versjon** for
samme år. Derfor kan en avviksrapport alltid navngi nøyaktig hva den
sammenligner mot: «Budsjett 2026 v1 (fastsatt)», ikke «budsjettet» som
stille kan ha endret seg siden sist noen så på det.

| Handling | Utkast | Fastsatt |
| --- | --- | --- |
| Endre linjer | ja | nei — lag ny versjon |
| Endre navn/notat | ja | nei |
| Fastsette | ja (enveis) | — |
| Forkaste | ja | nei — fastsatte budsjetter er historikk |

## Fortegn: budsjettet skrives slik det leses

Linjene lagres i **presentasjonsfortegn** — inntekt positiv, kostnad
positiv, som i resultatrapporten. Hovedbokens debet/kredit-konvensjon
gjelder bilag; et budsjett er ikke bilag, og å tvinge brukeren til å
skrive «−100 000» for planlagt salg ville vært en lekkasje fra
lagringsformatet ut i grensesnittet.

Faktiske tall konverteres med `regnmed_core::regnskap::presentasjon_ore`
— den samme regelen resultatrapporten bruker, ett sted i koden — før de
møter budsjettet, så sammenligningen skjer i ett rom.

Budsjettet dekker **resultatkontoer (klasse 3–8)**. Et forsøk på å
budsjettere en balansekonto avvises høyt; likviditetsbudsjett er
bevisst utenfor v1.

## Avviksrapporten

`avvik = faktisk − budsjett` per konto, gruppert i de samme NS
4102-seksjonene som resultatregnskapet (`regnmed-core::budsjett`, ren
funksjon med enhetstester). Rapporten viser hittil i år (t.o.m. valgt
måned — som standard inneværende måned for det løpende året, 12 for et
avsluttet), hele det budsjetterte året, og de tolv månedene per konto.

To valg verdt å nevne:

- **Fortegnet tolkes ikke.** Positivt avvik på en inntektskonto er
  bedre enn planlagt; på en kostnadskonto er det dyrere. Rapporten
  viser tallene og lar leseren om resten — den later ikke som den vet
  hva som er «bra».
- **En konto som bare finnes på én side blir med.** En kostnad ingen
  budsjetterte er nettopp det en avviksrapport er til for, og vises med
  null i budsjettkolonnen — aldri utelatt.

Standardvalget er **nyeste fastsatte** budsjett for året; finnes ingen
fastsatt, brukes nyeste utkast (så rapporten er nyttig mens man
fortsatt planlegger), og statusen står alltid i svaret. Et hvilket som
helst budsjett kan velges eksplisitt med `budget_id`.

## Fra fjoråret ±X %

`fra_ar` + `justering_bp` sår linjene fra det årets FAKTISKE tall,
skalert i basispunkter (500 = +5 %), avrundet halve øre bort fra null
(`regnmed_core::budsjett::juster_ore` — heltallsaritmetikk, ingen
flyttall nær penger). Resultatet er et **startpunkt**: et utkast
mennesket redigerer videre, ikke en prognose systemet står inne for.

## Endpoints

- `GET    /companies/{id}/budgets[?year=]` — versjonene m/ status og sum
- `POST   /companies/{id}/budgets` — nytt utkast (`year`, `navn`,
  valgfritt `fra_ar` + `justering_bp`); alltid neste versjon
- `GET    /companies/{id}/budgets/{bid}` — budsjettet m/ linjer
- `PUT    /companies/{id}/budgets/{bid}/lines` — erstatt linjene (utkast)
- `POST   /companies/{id}/budgets/{bid}/fastsett` — enveis
- `DELETE /companies/{id}/budgets/{bid}` — forkast utkast
- `GET    /companies/{id}/reports/avvik?year=&budget_id=&t_o_m=`

Lesing er åpen for alle tilgangsnivåer (revisor ser planen som alt
annet); endring krever bokføringstilgang.

## Portal

Rapporter → **Budsjett**: versjonslisten med status og hvem som laget
og fastsatte hver, et redigerbart rutenett (konto × 12 måneder) så
lenge budsjettet er utkast, «+ konto» med beløp per måned som fyller
hele raden, og avvikstabellen under — som alltid navngir versjonen den
måler mot.

## Tester

`crates/regnmed-core/src/budsjett.rs` har enhetstestene for regningen
(hittil-grensen, resultatfortegn, konto på bare én side, NS
4102-seksjonene, avrunding av justeringen).
`crates/regnmed-api/tests/budsjett.rs` kjører hele historien mot en
ekte database: «fra fjoråret +10 %» sår riktige linjer i
presentasjonsfortegn, utkastet redigeres fritt, balansekonto avvises,
fastsettelse fryser linjene og hindrer sletting, avviksrapporten
navngir versjonen, en revisjon blir v2 uten å overta rapporten før den
fastsettes, og et ubudsjettert bilag dukker opp i rapporten.

## Bevisst utenfor v1

Likviditetsbudsjett (resultatbudsjett først), rullerende prognoser, og
budsjett per avdeling/prosjekt — dimensjonene finnes (#37), så
utvidelsen er en kolonne på `budget_line` og et filter i rapporten, men
den bør vente til noen faktisk trenger den.
