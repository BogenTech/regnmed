# Kundevilkår for regnmed

> **UTKAST 2026-11-11 — ikke gjeldende, ikke juridisk gjennomgått.**
> Skrevet for å oppfylle §1.2 i Digdirs *Bruksvilkår for
> sluttbrukersystemleverandører i Altinn* (v1.0, 2025) og Skatteetatens
> *Bruksvilkår for tilgang til Skatteetatens tjenester – Leverandør av
> sluttbrukersystem*, som begge krever at leverandøren har en avtale med
> hver kunde som viderefører vilkårene. Må leses av advokat før den tas i
> bruk. Sporingstabellen i §14 viser hvilket krav hvert punkt dekker.
> Plassholdere står i `[hakeparenteser]`.

## 1. Parter og omfang

1.1 Disse vilkårene gjelder mellom **BogenTech AS** (org.nr. 935 115 086),
som utvikler og drifter regnskapssystemet regnmed på regnmed.no
(«regnmed», «vi»), og virksomheten som bruker regnmed for sitt eget
regnskap («kunden»).

1.2 Et regnskapsførerbyrå eller en revisor som bruker regnmed for en
klient, gjør det i kraft av sitt oppdrag fra klienten. Klienten er kunden
etter disse vilkårene. Byrået står inne for at det har fullmakt til å
akseptere vilkårene på klientens vegne, og for at oppdragsavtalen ikke
er i strid med dem.

1.3 Vilkårene består av denne teksten, databehandleravtalen i bilag 1 og
den til enhver tid gjeldende prislisten. Ved motstrid går
databehandleravtalen foran for behandling av personopplysninger, ellers
går denne teksten foran.

1.4 Kunden aksepterer vilkårene i regnmed, av en person med rett til å
binde kunden. Aksepten registreres med tidspunkt, person og
vilkårsversjon, og kan ikke endres i ettertid.

## 2. Tjenesten

2.1 regnmed er et regnskapssystem med bokføring, fakturering, lønn,
rapporter og innsending til offentlige myndigheter. Funksjonene som
finnes til enhver tid er beskrevet på regnmed.no og i dokumentasjonen.

2.2 Kunden eier sine regnskapsopplysninger. Vi bruker dem bare for å
levere tjenesten til kunden, se §5 og bilag 1.

2.3 Kunden er selv ansvarlig for at regnskapet er riktig og fullstendig,
og for å overholde bokføringsplikten, oppbevaringsplikten og fristene som
gjelder for kunden. regnmed er et verktøy for dette og overtar ikke
ansvaret.

## 3. Innsending til offentlige myndigheter

3.1 **Vi handler på kundens vegne.** Når kunden sender en melding eller
oppgave til en offentlig myndighet gjennom regnmed (for eksempel
mva-melding, a-melding, aksjonærregisteroppgave, skattemelding,
årsregnskap eller inntektsmelding), sender vi den på kundens vegne
gjennom Altinn eller myndighetens eget grensesnitt.

3.2 **Kunden er fullt ansvarlig overfor myndighetene.** Overfor
Digitaliseringsdirektoratet (Digdir), Skatteetaten og andre
tjenesteeiere er kunden fullt ut ansvarlig for det regnmed gjør i
Altinn og myndighetenes tjenester på kundens vegne, tilsvarende som om
kunden selv hadde utført handlingen. Kunden er ansvarlig for innholdet i
det som sendes inn.

3.3 **Fullmakten gis i Altinn, og kunden kan trekke den tilbake.**
Kunden gir regnmed tilgang ved å godkjenne en systembruker for regnmed i
Altinn med de tilgangspakkene innsendingen krever. Kunden kan når som
helst begrense eller slette systembrukeren i Altinn. Da stopper
innsendingen gjennom regnmed for det tilgangen dekket.

3.4 **Ingenting sendes uten en handling fra kunden.** regnmed sender
bare når en bruker eller integrasjon kunden har gitt rettigheten til, ber
om det. Hvem som sendte hva og når, logges og kan ses av kunden.

3.5 **Fristene er kundens.** Kunden er ansvarlig for å levere innen
fristene, også når innsending gjennom regnmed ikke er mulig — for
eksempel ved driftsstans hos oss, hos Altinn eller hos myndigheten, eller
fordi en funksjon ennå ikke finnes. Vi sier fra i regnmed når en
innsending feiler eller ikke kan gjøres, og kunden må da levere på annen
måte.

3.6 **Bare til offentlige oppgaver.** Opplysninger vi sender til eller
henter fra Altinn og myndighetenes tjenester, brukes bare til å oppfylle
kundens plikter overfor det offentlige og vise kunden resultatet — aldri
til våre egne formål, analyse, markedsføring eller salg, og aldri til
privatrettslig bruk.

3.7 **Kundens bruk må følge Altinns krav.** Kunden skal ikke bruke
regnmed til å omgå, overbelaste eller misbruke Altinn eller
myndighetenes tjenester, herunder skjermskraping eller automatiserte
kall utenom de funksjonene regnmed tilbyr. Vi kan begrense eller stenge
kundens tilgang til innsending dersom kundens bruk bryter med dette, se
§10.

## 4. Brukere og tilgangsstyring

4.1 Kunden bestemmer hvem som har tilgang til kundens selskap i regnmed
og med hvilke roller. Vi håndhever tilgangene i systemet.

4.2 Kunden skal gi hver bruker bare den tilgangen brukeren trenger, og
fjerne tilgang som ikke lenger trengs. Kunden skal gå gjennom brukere,
roller, integrasjoner og oppdrag minst én gang i året. [regnmed varsler
om gjennomgangen — ikke bygget.]

4.3 Innlogging skjer med personlig bruker. Brukere skal ikke dele
innlogging. Integrasjoner og agenter får egen maskinidentitet, og
attribueres som seg selv.

4.4 Vi har ingen leverandørvei inn i kundens hovedbok. Brukerstøtte får
bare tilgang dersom kunden selv inviterer eller gir oppdrag, og
plattformtilgang til kundens administrative data logges og er synlig for
kunden (docs/auth.md §8).

## 5. Personvern

5.1 Kunden er behandlingsansvarlig for personopplysningene i kundens
regnskap, lønn og innsendinger. Vi er databehandler og behandler dem
etter databehandleravtalen i bilag 1.

5.2 Vi er selv behandlingsansvarlig for opplysninger om kundens brukere
som vi trenger for å drive tjenesten (innlogging, fakturering av
abonnementet, sikkerhetslogger).

## 6. Sikkerhet

6.1 Vi skal sikre konfidensialitet, integritet og tilgjengelighet for
kundens opplysninger, herunder fødselsnummer og særlige kategorier av
personopplysninger, med tekniske og organisatoriske tiltak som er
beskrevet i bilag 1 punkt 6. Vi har planlagte og dokumenterte
internkontrolltiltak for dette.

6.2 Vi sørger for at ingen får tilgang til kundens opplysninger uten
tjenstlig behov.

## 7. Logging, innsyn og revisjon

7.1 Vi logger hvert kall regnmed gjør mot Altinn og myndighetenes
tjenester på kundens vegne: hva, når, for hvilken kunde, utløst av hvem,
og svaret. Loggene oppbevares i [oppbevaringstid].

7.2 Kunden aksepterer at Digdir, Skatteetaten eller tredjepart de
utpeker kan revidere at vi overholder deres bruksvilkår, og at de på
forespørsel kan få tilgang til loggene i 7.1 for feilsøking,
sikkerhetshendelser og kontroll. Vi utleverer ikke mer enn kontrollen
krever.

## 8. Avvik og sikkerhetsbrudd

8.1 Vi varsler kunden uten ugrunnet opphold om sikkerhetsbrudd som
berører kundens opplysninger, og om feil hos oss som kan ha gitt feil
innsending. Varselet sier hva som har skjedd, hva som er berørt og hva vi
gjør.

8.2 Vi varsler Digdir (servicedesk@altinn.no) om hendelser som kan
påvirke Altinn, Skatteetaten om avvik som kan være omfattet av
Skatteetatens behandlingsansvar, og bistår kunden med varsling til
Datatilsynet, slik regelverket og myndighetenes vilkår krever.

8.3 Kunden varsler oss uten ugrunnet opphold dersom kunden får mistanke
om at en innlogging, en integrasjon eller systembrukeren er misbrukt.

## 9. Drift og tilgjengelighet

9.1 Vi tilstreber høy tilgjengelighet, men garanterer ikke at regnmed,
Altinn eller myndighetenes tjenester er tilgjengelige til enhver tid.
Planlagt vedlikehold varsles i regnmed i rimelig tid.

9.2 Altinn og myndighetene kan endre sine grensesnitt, og kan i noen
tilfeller gjøre det på kort varsel. Vi tilpasser regnmed, men kan ikke
love at en innsending virker i perioden før tilpasningen er på plass.

## 10. Stans og begrensning

10.1 Digdir og Skatteetaten kan stenge regnmeds tilgang til Altinn og
sine tjenester, helt eller delvis, med eller uten varsel. Da stanser
innsendingen gjennom regnmed. Vi varsler kunden så raskt vi kan.
Bokføringen og kundens tilgang til egne opplysninger berøres ikke.

10.2 Vi kan stanse kundens tilgang til innsending dersom kundens bruk
bryter med §3.6–3.7, eller dersom Digdir eller Skatteetaten krever det.
Vi varsler kunden først når det er mulig og gir kunden anledning til å
rette forholdet.

10.3 Sperret abonnement stanser endringer, men aldri kundens tilgang til
å lese og eksportere egne opplysninger (docs/abonnement.md).

## 11. Ansvar

11.1 Vi er ansvarlige for at regnmed sender det kunden har bedt om, slik
kunden har registrert det. Vi er ikke ansvarlige for innholdet kunden har
registrert, for kundens valg, eller for tap som skyldes Altinn,
myndighetenes tjenester eller annen tredjepart.

11.2 Vårt samlede ansvar overfor kunden er begrenset til [beløp, f.eks.
det kunden har betalt for de siste tolv månedene]. Begrensningen gjelder
ikke ved forsett eller grov uaktsomhet.

11.3 Vi er ikke ansvarlige for indirekte tap, herunder tapt fortjeneste,
tilleggsskatt, gebyrer eller tvangsmulkt, med mindre tapet skyldes
forsett eller grov uaktsomhet hos oss. [Avklares: om forsinkelsesgebyr
som skyldes feil hos oss skal dekkes.]

## 12. Endringer, overdragelse og opphør

12.1 Vi kan endre vilkårene. Endringer varsles i regnmed og på e-post
minst [30] dager før de trer i kraft. Endringer som Digdir, Skatteetaten
eller regelverket krever, kan tre i kraft på det tidspunktet kravet
gjelder fra. Kunden kan si opp dersom kunden ikke godtar en endring.

12.2 Dersom eierskapet til regnmed overdras, varsler vi kunden og
Skatteetaten. Kunden kan da si opp.

12.3 Kunden kan si opp når som helst etter reglene for abonnementet. Ved
opphør stanser vi alle kall på kundens vegne, og kunden bør slette
systembrukeren for regnmed i Altinn.

12.4 Etter opphør kan kunden eksportere sine opplysninger (SAF-T,
dokumenter) i [periode]. Opplysninger som kunden har oppbevaringsplikt
for, slettes ikke før oppbevaringstiden er ute, med mindre kunden har
tatt dem med seg og ber om sletting.

## 13. Lovvalg og tvister

Norsk rett gjelder. Tvister søkes løst i minnelighet, ellers ved
[BogenTechs hjemting] som avtalt verneting.

## 14. Sporingstabell: hvilket krav hvert punkt dekker

| Krav | Kilde | Dekket i |
| --- | --- | --- |
| Avtale med kunden som ivaretar bruksvilkårene | Digdir §1.2; Skatteetaten «Tilgang til tjenestene» | §1, hele teksten |
| Kunden fullt ansvarlig for leverandørens handlinger i Altinn | Digdir §1.2 | §3.2 |
| Leverandøren ansvarlig for tilgangsstyring i eget system | Digdir §1.2; Skatteetaten | §4.1 |
| Sikre at sluttbruker ikke handler i strid med Altinns krav | Digdir §1.2 | §3.7, §10.2 |
| Bare offentlig oppgaveløsning, ingen privatrettslig bruk | Digdir §1.2.1 | §3.6 |
| Ikke bruke tilgangen eller opplysningene til egne formål | Skatteetaten «Formål», «Tilgang til tjenestene» | §3.6, §2.2 |
| Databehandleravtale med kundene | Skatteetaten «Formål», «Tilgang til tjenestene» | §5, bilag 1 |
| Revidere tilganger fast | Skatteetaten «Tilgang til tjenestene» | §4.2 |
| Konfidensialitet, integritet, tilgjengelighet; fnr og særlige kategorier | Digdir §5; Skatteetaten «Personvern og informasjonssikkerhet» | §6, bilag 1 pkt. 6 |
| Internkontroll | Skatteetaten «Personvern og informasjonssikkerhet» | §6.1 |
| Brukerstøtte til egne sluttbrukere | Skatteetaten «Brukerstøtte» | §9, bilag 1 pkt. 9 |
| Revisjon og tilgang til logger | Digdir §6 | §7 |
| Varsling av hendelser | Digdir §1.2.8; Skatteetaten «Hendelser og sikkerhetsbrudd» | §8 |
| Frakobling ved mislighold | Digdir §1.2.8, §7; Skatteetaten «Mislighold» | §10 |
| Digdir ensidig endring | Digdir §8 | §12.1 |
| Melde eierskifte | Skatteetaten «Tilgang til tjenestene» | §12.2 |
| Digdir fraskriver seg ansvar for feil i Altinn | Digdir §7.3.2 | §11.1 |

---

## Bilag 1: Databehandleravtale

Etter personvernforordningen (GDPR) art. 28 nr. 3.

**1. Parter.** Kunden er behandlingsansvarlig. BogenTech AS er
databehandler.

**2. Formål og art.** Å levere regnmed til kunden: bokføring,
fakturering, lønn, rapporter, lagring av bilag og innsending til
offentlige myndigheter på kundens vegne.

**3. Varighet.** Så lenge kunden bruker regnmed, og deretter så lenge
opplysningene oppbevares etter §12.4.

**4. Kategorier av registrerte og opplysninger.**

| Registrerte | Opplysninger |
| --- | --- |
| Kundens ansatte | navn, fødselsnummer/D-nummer, kontaktinformasjon, arbeidsforhold, lønn, skattetrekk, feriepenger, kontonummer, fravær som inngår i lønn |
| Kundens ansatte (inntektsmelding, når bygget) | opplysninger om sykefravær og foreldrepermisjon — **særlig kategori (helse)** |
| Aksjonærer | navn, fødselsnummer, adresse, aksjebeholdning, utbytte |
| Kunder og leverandører som er personer | navn, adresse, e-post, kontonummer, transaksjoner |
| Kundens brukere | navn, e-post, handlinger i regnmed |
| Andre i bilag | det bilaget inneholder |

**5. Instruks.** Vi behandler opplysningene bare etter kundens
dokumenterte instruks, som er disse vilkårene og kundens bruk av
funksjonene i regnmed. Vi sier fra dersom vi mener en instruks er i
strid med regelverket.

**6. Sikkerhet (art. 32).** Blant annet: kryptert transport (TLS);
innlogging gjennom OpenID Connect; rollebasert tilgang håndhevet i én
vakt (docs/auth.md); append-only hovedbok med hashkjede og ekstern
forankring (docs/ledger.md, docs/anchoring.md); fødselsnummer vises ikke
i portalen og brukes bare der innsendingen krever det; ingen hemmeligheter
i kildekoden (docs/secrets.md); nattlig sikkerhetskopi med ukentlig
verifisert gjenoppretting (docs/deploy.md). [Supplere: kryptering i
hvile, tilgangskontroll for driftspersonell.]

**7. Taushetsplikt.** Alle som behandler opplysningene på våre vegne, har
taushetsplikt.

**8. Underdatabehandlere.** Kunden godkjenner underdatabehandlerne under.
Vi varsler kunden minst [30] dager før vi bytter eller legger til en,
og kunden kan protestere og si opp.

| Underdatabehandler | Formål | Sted |
| --- | --- | --- |
| Hetzner Online GmbH | drift av servere, database og dokumentlagring | Helsingfors, Finland (EØS) |
| [e-postleverandør for utsendelse] | utsendelse av faktura, purring og varsler | [sted] |
| [andre] | | |

En LLM-leverandør kunden selv velger for en agent, er kundens valg og
kundens databehandler, ikke vår underdatabehandler (docs/agent.md). Stripe
behandler betalingen av kundens abonnement hos oss og er ikke
underdatabehandler for opplysningene i kundens regnskap.

**9. Bistand.** Vi bistår kunden med å besvare henvendelser fra
registrerte (innsyn, retting) og med å oppfylle art. 32–36, i den grad
det er rimelig ut fra behandlingens art. Spørsmål om personopplysningene
håndteres av vår brukerstøtte og meldes ikke som teknisk feil til
myndighetene.

**10. Avvik.** Vi varsler kunden uten ugrunnet opphold, og senest innen
[48] timer etter at vi ble kjent med et brudd på personopplysningssikkerheten,
med opplysningene kunden trenger for å varsle Datatilsynet.

**11. Overføring utenfor EØS.** Skjer ikke uten kundens skriftlige
samtykke og et gyldig overføringsgrunnlag.

**12. Ved opphør.** Opplysningene leveres tilbake eller slettes etter
§12.4. Opplysninger som kunden har lovpålagt oppbevaringsplikt for,
slettes ikke før plikten er ute.

**13. Revisjon.** Vi gir kunden opplysningene som trengs for å vise at
avtalen overholdes, og gir adgang til revisjon etter rimelig varsel.
