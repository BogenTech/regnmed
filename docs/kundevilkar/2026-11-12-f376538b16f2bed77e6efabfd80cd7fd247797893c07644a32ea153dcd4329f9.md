# Kundevilkår for regnmed

> **UTKAST 2026-11-11, revidert samme dag — ikke gjeldende, ikke
> juridisk gjennomgått.** Skrevet for å oppfylle §1.2 i Digdirs
> *Bruksvilkår for sluttbrukersystemleverandører i Altinn* (v1.0, 2025)
> og Skatteetatens *Bruksvilkår for tilgang til Skatteetatens tjenester –
> Leverandør av sluttbrukersystem*, som begge krever at leverandøren har
> en avtale med hver kunde som viderefører vilkårene — og for å gi
> BogenTech de vernene en leverandør av en rimelig standardtjeneste
> trenger. Må leses av advokat før den tas i bruk; spørsmålene til
> advokaten står i §17. Sporingstabellen i §16 viser hvilket krav hvert
> punkt dekker.

## Aksept (hvordan regnmed håndhever §1.4)

Teksten i denne fila **er** vilkårene: binæren serverer den uendret på
`/vilkar` (ren tekst) og `/terms` (JSON), og versjonen er SHA-256 av
teksten. En administrator (`SELSKAP_ADMIN`, som aldri kan gis til en
integrasjon) godtar den under Administrasjon → Firmaopplysninger; aksepten
lagres innsettings-bart i `terms_acceptance` (migrasjon 0077) med
selskap, person, navn, tidspunkt og hashen. Hver innsending til
myndighetene (a-melding, aksjonærregisteroppgave) nekter til gjeldende
versjon er godtatt.

**Endres denne fila, endres versjonen**, og hvert selskap må godta på
nytt før neste innsending. Det gjelder også skrivefeil — det er prisen
for at «godtatt» alltid betyr én bestemt tekst.

**Hver versjon som har gjeldt, arkiveres** i `docs/kundevilkar/` som
`<dato>-<sha256>.md` (datoen den trer i kraft), og serveres på
`/vilkar/<sha256>`; `/terms/versions` lister dem. En kunde som godtok en
eldre tekst kan dermed alltid lese nettopp den, og selskapets egen
aksepthistorikk står i `GET /companies/{id}/terms`. Testen
`the_current_text_is_archived` feiler hvis denne fila endres uten at den
nye teksten arkiveres, og hvis en arkivert tekst redigeres i etterkant.

**Registeret i §7.1** er innsendingssporene selv (migrasjon 0074–0076),
som skrives FØR hvert kall og ikke kan endres eller slettes — ikke en
egen logg ved siden av. `GET /companies/{id}/authority-calls` viser dem
samlet: skjema, hva det gjaldt, hvem som ba om det, når, og hva som kom
tilbake (kvittering, feil eller myndighetens svar).

**Tilgangsgjennomgangen i §4.2** er kundens plikt; regnmed håndhever
tilgangene (§4.1) men gjennomfører ikke gjennomgangen for kunden.

## 1. Parter og omfang

1.1 Disse vilkårene gjelder mellom **BogenTech AS** (org.nr. 935 115 086),
som utvikler og drifter regnskapssystemet regnmed på regnmed.no
(«regnmed», «vi»), og virksomheten som bruker regnmed for sitt eget
regnskap («kunden»). regnmed er en tjeneste for næringsdrivende og
andre virksomheter, ikke for forbrukere.

1.2 Et regnskapsførerbyrå eller en revisor som bruker regnmed for en
klient, gjør det i kraft av sitt oppdrag fra klienten. Klienten er kunden
etter disse vilkårene. Byrået står inne for at det har fullmakt til å
akseptere vilkårene på klientens vegne, og for at oppdragsavtalen ikke
er i strid med dem. Byrået holder oss skadesløse dersom fullmakten
mangler.

1.3 Vilkårene består av denne teksten, databehandleravtalen i bilag 1 og
den til enhver tid gjeldende prislisten på regnmed.no. Ved motstrid går
databehandleravtalen foran for behandling av personopplysninger, ellers
går denne teksten foran.

1.4 Kunden aksepterer vilkårene i regnmed, av en person med rett til å
binde kunden. Personen som aksepterer, står inne for at hen har denne
retten. Aksepten registreres med tidspunkt, person og vilkårsversjon, og
kan ikke endres i ettertid.

1.5 **Dette er tjenestevilkår, ikke en programvarelisens.** Kildekoden
til regnmed er åpen under AGPL-3.0. Kunden trenger ingen lisens fra oss
for å bruke tjenesten, og lisensen gir ingen rettigheter eller plikter
etter disse vilkårene. Tjenesten, driften og navnet regnmed tilhører
BogenTech.

## 2. Tjenesten

2.1 regnmed er et regnskapssystem med bokføring, fakturering, lønn,
rapporter og innsending til offentlige myndigheter. Funksjonene som
finnes til enhver tid er beskrevet på regnmed.no og i dokumentasjonen.
Vi kan videreutvikle, endre og fjerne funksjoner. Fjerner vi en funksjon
kunden bruker for å oppfylle en lovpålagt plikt, varsler vi minst 30
dager før.

2.2 Kunden eier sine regnskapsopplysninger. Vi bruker dem bare for å
levere tjenesten til kunden, se §5 og bilag 1.

2.3 **Kunden er ansvarlig for regnskapet.** Kunden er selv ansvarlig for
at regnskapet er riktig og fullstendig, og for å overholde
bokføringsplikten, oppbevaringsplikten og fristene som gjelder for
kunden. regnmed er et verktøy for dette og overtar ikke ansvaret.

2.4 **Kunden kontrollerer før det brukes.** Beregninger og forslag i
regnmed — blant annet mva, lønn, forskuddstrekk, arbeidsgiveravgift,
feriepenger, avskrivninger, purregebyr og renter, bokføringsforslag og
kontoforslag — er hjelpemidler. Kunden skal kontrollere tallene og
innholdet før det bokføres, faktureres, utbetales eller sendes inn, og
skal følge opp kvitteringer og tilbakemeldinger fra myndighetene.

2.5 **Ingen rådgivning.** regnmed gir ikke regnskaps-, skatte-, avgifts-
eller juridisk rådgivning. Det gjelder også forslag fra regnmed og svar
fra en agent kunden har koblet til.

2.6 **Kundens integrasjoner og agenter er kundens.** Handlinger utført av
en integrasjon, en agent eller en språkmodell kunden har gitt tilgang,
regnes som kundens egne handlinger. Kunden velger og er ansvarlig for
leverandøren av slike verktøy og for det de gjør.

2.7 **Tjenesten leveres som den er.** Vi leverer tjenesten med rimelig
omhu og fagkunnskap, men lover ikke at den er feilfri, at den passer for
et bestemt formål, eller at den til enhver tid dekker alle regler som
gjelder for kunden. Funksjoner som er merket som test eller beta, leveres
uten noen form for garanti.

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
om det. Hvem som sendte hva og når, registreres og kan ses av kunden.

3.5 **Fristene er kundens.** Kunden er ansvarlig for å levere innen
fristene, også når innsending gjennom regnmed ikke er mulig — for
eksempel ved driftsstans hos oss, hos Altinn eller hos myndigheten, eller
fordi en funksjon ennå ikke finnes. Vi sier fra i regnmed når en
innsending feiler eller ikke kan gjøres, og kunden må da levere på annen
måte. Filene kan alltid lastes ned for levering på annen måte der
regnmed kan lage dem.

3.6 **Kvitteringen er kundens dokumentasjon.** En innsending er levert
når myndigheten har gitt kvittering. Kvitteringen vises i regnmed. At en
innsending er levert, betyr ikke at myndigheten har godkjent innholdet.

3.7 **Bare til offentlige oppgaver.** Opplysninger vi sender til eller
henter fra Altinn og myndighetenes tjenester, brukes bare til å oppfylle
kundens plikter overfor det offentlige og vise kunden resultatet — aldri
til våre egne formål, analyse, markedsføring eller salg, og aldri til
privatrettslig bruk.

3.8 **Kundens bruk må følge Altinns krav.** Kunden skal ikke bruke
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
roller, integrasjoner og oppdrag minst én gang i året.

4.3 Innlogging skjer med personlig bruker. Brukere skal ikke dele
innlogging. Kunden er ansvarlig for alt som gjøres med kundens brukere,
integrasjoner og nøkler. Integrasjoner og agenter får egen
maskinidentitet, og registreres som seg selv.

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

5.3 Kunden står inne for at kunden har rettslig grunnlag for
personopplysningene kunden legger inn i regnmed.

## 6. Sikkerhet

6.1 Vi skal sikre konfidensialitet, integritet og tilgjengelighet for
kundens opplysninger, herunder fødselsnummer og særlige kategorier av
personopplysninger, med tekniske og organisatoriske tiltak som er
beskrevet i bilag 1 punkt 6. Vi har planlagte og dokumenterte
internkontrolltiltak for dette.

6.2 Vi sørger for at ingen får tilgang til kundens opplysninger uten
tjenstlig behov.

6.3 Kundens opplysninger lagres og driftes i EØS (bilag 1 punkt 8).

## 7. Registrering, innsyn og revisjon

7.1 Vi registrerer hver innsending regnmed gjør til Altinn og
myndighetenes tjenester på kundens vegne: hva som ble sendt, når, for
hvilken kunde, hvem som ba om det, og svaret. Registreringen kan ikke
endres, og oppbevares i minst fem år etter utgangen av regnskapsåret
innsendingen gjelder.

7.2 Kunden aksepterer at Digdir, Skatteetaten eller tredjepart de
utpeker kan revidere at vi overholder deres bruksvilkår, og at de på
forespørsel kan få tilgang til registreringene i 7.1 for feilsøking,
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

## 9. Drift, tilgjengelighet og brukerstøtte

9.1 Vi tilstreber høy tilgjengelighet, men garanterer ikke at regnmed,
Altinn eller myndighetenes tjenester er tilgjengelige til enhver tid.
Planlagt vedlikehold varsles i regnmed i rimelig tid.

9.2 Altinn og myndighetene kan endre sine grensesnitt, og kan i noen
tilfeller gjøre det på kort varsel. Vi tilpasser regnmed, men kan ikke
love at en innsending virker i perioden før tilpasningen er på plass.

9.3 Brukerstøtte gis i det omfanget kundens abonnement omfatter
(prislisten). Vi gir brukerstøtte om bruken av regnmed, ikke om kundens
regnskap. Vi lover ingen bestemt svartid.

9.4 Vi tar sikkerhetskopi av kundens opplysninger og prøver
gjenopprettingen jevnlig (bilag 1 punkt 6). Kunden kan når som helst
eksportere sine opplysninger, og bør gjøre det jevnlig.

## 10. Stans og begrensning

10.1 Digdir og Skatteetaten kan stenge regnmeds tilgang til Altinn og
sine tjenester, helt eller delvis, med eller uten varsel. Da stanser
innsendingen gjennom regnmed. Vi varsler kunden så raskt vi kan.
Bokføringen og kundens tilgang til egne opplysninger berøres ikke.

10.2 Vi kan stanse kundens tilgang til innsending dersom kundens bruk
bryter med §3.7–3.8, eller dersom Digdir eller Skatteetaten krever det.
Vi varsler kunden først når det er mulig og gir kunden anledning til å
rette forholdet.

10.3 Vi kan stanse hele eller deler av tjenesten straks dersom det er
nødvendig for å avverge eller begrense et sikkerhetsbrudd eller
alvorlig skade på tjenesten, andre kunder eller tredjepart.

10.4 Sperret abonnement stanser endringer, men aldri kundens tilgang til
å lese og eksportere egne opplysninger så lenge avtalen består
(docs/abonnement.md).

## 11. Pris og betaling

11.1 Prisene står i prislisten på regnmed.no. Vi kan endre prisene med
minst 30 dagers varsel i regnmed og på e-post. En prisendring gjelder
fra første fakturaperiode etter at varselet er ute.

11.2 Betaling skjer med kort eller faktura etter reglene for
abonnementet. Ved forsinket betaling kan vi kreve forsinkelsesrente og
purregebyr etter lov, og sperre abonnementet etter §10.4.

## 12. Ansvar

12.1 **Avhjelp først.** Er tjenesten mangelfull, skal kunden reklamere
skriftlig uten ugrunnet opphold etter at kunden oppdaget eller burde ha
oppdaget mangelen, og senest tre måneder etter. Vi har rett og plikt til
å rette mangelen innen rimelig tid. Reklamerer kunden ikke i tide, tapes
retten til å gjøre mangelen gjeldende.

12.2 Vi er ansvarlige for at regnmed sender det kunden har bedt om, slik
kunden har registrert det. Vi er ikke ansvarlige for innholdet kunden har
registrert, for kundens valg, for forhold kunden skulle ha oppdaget ved
kontrollen etter §2.4, eller for tap som skyldes Altinn, myndighetenes
tjenester eller annen tredjepart.

12.3 **Indirekte tap og offentlige sanksjoner.** Vi er ikke ansvarlige for
indirekte tap, herunder tapt fortjeneste, tapte data utover det som kan
gjenopprettes fra siste sikkerhetskopi, krav fra tredjepart, og
tilleggsskatt, overtredelsesgebyr, forsinkelsesgebyr, tvangsmulkt og
renter som myndighetene ilegger kunden.

12.4 **Ansvarsgrense.** Vårt samlede ansvar overfor kunden, uansett
grunnlag og også etter bilag 1, er for hvert kalenderår begrenset til det
kunden har betalt oss for tjenesten de siste tolv månedene før
hendelsen som utløste kravet.

12.5 Begrensningene i 12.3 og 12.4 gjelder ikke ved forsett eller grov
uaktsomhet hos oss, eller der ufravikelig lov sier noe annet.

12.6 **Skadesløsholdelse.** Kunden holder oss skadesløse for krav fra
tredjepart — herunder Digdir, Skatteetaten, andre myndigheter og
registrerte — som skyldes innholdet kunden har registrert eller sendt
inn, kundens brudd på disse vilkårene, eller misbruk av kundens brukere
eller integrasjoner.

12.7 **Force majeure.** Ingen av partene er ansvarlige for forsinkelse
eller manglende oppfyllelse som skyldes forhold utenfor partens kontroll
som parten ikke burde ha forutsett eller kunne ha unngått, herunder
svikt hos Altinn, myndighetene, strømleverandører, nettverk eller
driftsleverandører, cyberangrep tross forsvarlige tiltak, og pålegg fra
myndighetene.

## 13. Taushetsplikt

Partene skal behandle opplysninger om den andre parten som de får
kjennskap til gjennom avtalen, konfidensielt. Det gjelder ikke
opplysninger som er offentlige, eller som en part må gi etter lov,
pålegg fra myndighet eller §7.2.

## 14. Endringer, overdragelse og opphør

14.1 Vi kan endre vilkårene. Endringer varsles i regnmed og på e-post
minst 30 dager før de trer i kraft. Endringer som Digdir, Skatteetaten
eller regelverket krever, kan tre i kraft på det tidspunktet kravet
gjelder fra. Kunden kan si opp dersom kunden ikke godtar en endring.

14.2 Vi kan overdra avtalen til den som overtar driften av regnmed, eller
til et selskap i samme konsern. Vi varsler kunden og Skatteetaten, og
kunden kan da si opp. Kunden kan ikke overdra avtalen uten vårt
samtykke.

14.3 Kunden kan si opp når som helst etter reglene for abonnementet.

14.4 Vi kan si opp avtalen med tre måneders skriftlig varsel. Vi kan
heve avtalen straks ved vesentlig mislighold, herunder brudd på §3.7–3.8
eller betalingsmislighold som ikke er rettet etter varsel. Legger vi ned
tjenesten, varsler vi minst seks måneder før.

14.5 Ved opphør stanser vi alle kall på kundens vegne, og kunden bør
slette systembrukeren for regnmed i Altinn.

14.6 **Eksport og sletting etter opphør.** I 90 dager etter opphør kan
kunden lese og eksportere sine opplysninger (SAF-T, dokumenter,
rapporter). **Kunden er selv ansvarlig for å oppbevare regnskapet i hele
oppbevaringstiden etter bokføringsloven**, og må eksportere det før
fristen. Etter 90 dager sletter vi kundens opplysninger, etter å ha
varslet kunden minst 30 dager før. Kopier i sikkerhetskopiene forsvinner
når kopiene roteres ut, innen 30 dager. Registreringene i §7.1 slettes
ikke før fristen der er ute. Vil kunden at vi skal oppbevare regnskapet
lenger, må abonnementet løpe.

## 15. Varsler, lovvalg og tvister

15.1 Varsler etter disse vilkårene gis i regnmed og på e-post til
adressen kunden har registrert. Kunden er ansvarlig for at adressen er
riktig.

15.2 Norsk rett gjelder. Tvister søkes løst i minnelighet, ellers ved
tingretten der BogenTech AS har sitt forretningskontor, som avtalt
verneting.

## 16. Sporingstabell: hvilket krav hvert punkt dekker

| Krav | Kilde | Dekket i |
| --- | --- | --- |
| Avtale med kunden som ivaretar bruksvilkårene | Digdir §1.2; Skatteetaten «Tilgang til tjenestene» | §1, hele teksten |
| Kunden fullt ansvarlig for leverandørens handlinger i Altinn | Digdir §1.2 | §3.2 |
| Leverandøren ansvarlig for tilgangsstyring i eget system | Digdir §1.2; Skatteetaten | §4.1 |
| Sikre at sluttbruker ikke handler i strid med Altinns krav | Digdir §1.2 | §3.8, §10.2 |
| Bare offentlig oppgaveløsning, ingen privatrettslig bruk | Digdir §1.2.1 | §3.7 |
| Ikke bruke tilgangen eller opplysningene til egne formål | Skatteetaten «Formål», «Tilgang til tjenestene» | §3.7, §2.2 |
| Databehandleravtale med kundene | Skatteetaten «Formål», «Tilgang til tjenestene» | §5, bilag 1 |
| Revidere tilganger fast | Skatteetaten «Tilgang til tjenestene» | §4.2 |
| Konfidensialitet, integritet, tilgjengelighet; fnr og særlige kategorier | Digdir §5; Skatteetaten «Personvern og informasjonssikkerhet» | §6, bilag 1 pkt. 6 |
| Internkontroll | Skatteetaten «Personvern og informasjonssikkerhet» | §6.1 |
| Brukerstøtte til egne sluttbrukere | Skatteetaten «Brukerstøtte» | §9.3, bilag 1 pkt. 9 |
| Revisjon og tilgang til logger | Digdir §6 | §7 |
| Varsling av hendelser | Digdir §1.2.8; Skatteetaten «Hendelser og sikkerhetsbrudd» | §8 |
| Frakobling ved mislighold | Digdir §1.2.8, §7; Skatteetaten «Mislighold» | §10 |
| Digdir ensidig endring | Digdir §8 | §14.1 |
| Melde eierskifte | Skatteetaten «Tilgang til tjenestene» | §14.2 |
| Digdir fraskriver seg ansvar for feil i Altinn | Digdir §7.3.2 | §12.2 |

## 17. Til advokaten: spørsmålene vi trenger svar på

Valgene under er tatt for å beskytte BogenTech så langt vi tror det
holder. Hvert av dem er en vurdering, ikke en sannhet.

1. **Ansvarsgrensen (§12.4)** er tolv måneders vederlag, altså noen hundre
   kroner for de fleste kunder. Holder den mot avtaleloven §36 i et
   standardvilkår mellom næringsdrivende, eller bør den ha et
   minstebeløp for å stå seg?
2. **Offentlige sanksjoner (§12.3).** Vi utelukker tilleggsskatt,
   forsinkelsesgebyr og liknende også når feilen er vår, så lenge den
   ikke er grovt uaktsom. Er det rimelig nok til å stå seg, når en
   innsendingstjeneste er det kunden kjøper?
3. **Reklamasjonsfristen (§12.1)**, tre måneder med rettstap. Er den
   gyldig som avtalt bortfallsfrist mellom næringsdrivende, og forholder
   den seg riktig til foreldelsesloven?
4. **Sletting etter opphør (§14.6).** Vi legger oppbevaringsplikten etter
   bokføringsloven §13 fullt på kunden og sletter etter 90 dager. Er det
   forsvarlig, og er varselet tilstrekkelig?
5. **Byråets fullmakt (§1.2).** Er det nok at byrået står inne for
   fullmakten, eller må klienten selv akseptere?
6. **Skadesløsholdelsen (§12.6)** — rekkevidde og om den bør speile
   ansvarsgrensen.
7. **Tailscale.** Trafikken mellom regnmed og e-postreléet går gjennom et
   kryptert nettverk fra Tailscale Inc. Tailscale ser ikke innholdet, og
   vi har derfor ikke ført det som underdatabehandler. Er det riktig?
8. **Verneting (§15.2)** og om voldgift er bedre for oss.

---

## Bilag 1: Databehandleravtale

Etter personvernforordningen (GDPR) art. 28 nr. 3.

**1. Parter.** Kunden er behandlingsansvarlig. BogenTech AS er
databehandler.

**2. Formål og art.** Å levere regnmed til kunden: bokføring,
fakturering, lønn, rapporter, lagring av bilag og innsending til
offentlige myndigheter på kundens vegne.

**3. Varighet.** Så lenge kunden bruker regnmed, og deretter til
opplysningene er slettet etter vilkårene §14.6.

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
i kildekoden (docs/secrets.md); kontinuerlig arkivering av databasen og
nattlig sikkerhetskopi med 30 dagers oppbevaring, og jevnlig verifisert
gjenoppretting (docs/deploy.md).

**7. Taushetsplikt.** Alle som behandler opplysningene på våre vegne, har
taushetsplikt.

**8. Underdatabehandlere.** Kunden godkjenner underdatabehandlerne under.
Vi varsler kunden minst 30 dager før vi bytter eller legger til en, og
kunden kan protestere og si opp.

| Underdatabehandler | Formål | Sted |
| --- | --- | --- |
| Hetzner Online GmbH | servere, database, dokumentlagring og sikkerhetskopi | Helsingfors, Finland (EØS) |
| ServeTheWorld AS | virtuell server for utsendelse av e-post (faktura, purring, varsler) | Norge |

En LLM-leverandør kunden selv velger for en agent, er kundens valg og
kundens databehandler, ikke vår underdatabehandler (docs/agent.md). Stripe
behandler betalingen av kundens abonnement hos oss og er ikke
underdatabehandler for opplysningene i kundens regnskap. Offentlige
myndigheter som mottar innsendinger, er mottakere, ikke
underdatabehandlere.

**9. Bistand.** Vi bistår kunden med å besvare henvendelser fra
registrerte (innsyn, retting) og med å oppfylle art. 32–36, i den grad
det er rimelig ut fra behandlingens art. Bistand utover det regnmed gjør
av seg selv, kan vi fakturere etter medgått tid. Spørsmål om
personopplysningene håndteres av vår brukerstøtte og meldes ikke som
teknisk feil til myndighetene.

**10. Avvik.** Vi varsler kunden uten ugrunnet opphold, og så langt det er
mulig innen 48 timer etter at vi ble kjent med et brudd på
personopplysningssikkerheten, med opplysningene kunden trenger for å
varsle Datatilsynet.

**11. Overføring utenfor EØS.** Skjer ikke uten kundens skriftlige
samtykke og et gyldig overføringsgrunnlag.

**12. Ved opphør.** Opplysningene gjøres tilgjengelige for eksport og
slettes deretter etter vilkårene §14.6.

**13. Revisjon.** Vi gir kunden opplysningene som trengs for å vise at
avtalen overholdes. Revisjon hos oss skjer primært skriftlig, ellers med
minst 30 dagers varsel, høyst én gang i året med mindre et
sikkerhetsbrudd gir grunn til mer, under taushetsplikt og for kundens
regning.
