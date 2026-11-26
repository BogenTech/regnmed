# Kundevilkårene: grunnlag for juridisk gjennomgang

Arbeidsdokument for gjennomgangen av `docs/kundevilkar.md` (#185). Det er
**ikke** en del av vilkårene: vilkårsteksten er hashet, og alt som står i
den må kunden godta (`docs/vilkar-teknisk.md`). Derfor står sporingen og
spørsmålene her.

Vilkårene er skrevet for å oppfylle §1.2 i Digdirs *Bruksvilkår for
sluttbrukersystemleverandører i Altinn* (v1.0, 2025) og Skatteetatens
*Bruksvilkår for tilgang til Skatteetatens tjenester – Leverandør av
sluttbrukersystem*, som begge krever at leverandøren har en avtale med
hver kunde som viderefører vilkårene — og for å gi BogenTech de vernene
en leverandør av en rimelig standardtjeneste trenger. **De er ikke
juridisk gjennomgått.** Punktnumrene under viser til `docs/kundevilkar.md`.

## Sporingstabell: hvilket krav hvert punkt dekker

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

## Til advokaten: spørsmålene vi trenger svar på

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
9. **Aksept av varslede vilkår (§14.1).** En ny versjon varsles før den
   gjelder. Et selskap som bare har godtatt den nye versjonen, får sende
   inn også i varslingsperioden, mens den gamle teksten gjelder — uten å
   ha godtatt den gamle (`docs/vilkar-teknisk.md`). Binder aksepten av
   den nye teksten kunden også for perioden før den trer i kraft, eller
   må kunden godta den gjeldende i tillegg?
