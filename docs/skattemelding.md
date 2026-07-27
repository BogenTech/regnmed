# Skattemelding og næringsspesifikasjon (#11) — kartlagt, ikke bygget

**Ingenting av dette er bygget.** Denne filen finnes fordi undersøkelsen
avdekket to ting som endrer hva #11 faktisk er, og som er dyre å finne
ut av på nytt: innsendingen går ikke på maskinporten-skinnen i det hele
tatt, og hvilken XSD-versjon som gjelder inneværende inntektsår er ikke
publisert noe sted vi har funnet.

Samme form som docs/regelverk.md-avsnittet om avvikende regnskapsår
(#52): antakelsen navngis, kostnaden kartlegges, og koden skrives ikke
på en gjetning.

## Det som stopper saken: innsending krever ID-porten

Skatteetaten skriver det selv, om overgangen til Altinn 3 for
skattemelding og mva:

> «Validering og innsending må fortsatt gjøres med ID-porten»

For inntektsår 2025 kan et sluttbrukersystem bruke **Maskinporten** til
å *hente* gjeldende skattemelding og PDF av fastsatt skattemelding —
men ikke til å validere eller sende inn.

Det er ikke et scope vi kan bestille. ID-porten er innlogging av et
**menneske**, ikke en maskinidentitet, og hele
Maskinporten-fundamentet (docs/gov.md, `crates/regnmed-gov`) er bygget
for det motsatte. Sammenlign med de andre innsendingene:

| Innsending | Skinne |
| --- | --- |
| Mva-melding | Maskinporten + Altinn 3-instansflyt |
| Aksjonærregisteroppgaven (RF-1086) | Maskinporten + Altinn systembruker |
| **Skattemelding + næringsspesifikasjon** | **ID-porten — en innlogget person** |

**Derfor henger #11 sammen med #26** (ID-porten-føderering via regnid) på
en måte ROADMAP-en ikke sa: #26 er ikke bare en innloggingsbekvemmelighet,
den ligger på veien til å kunne sende inn skattemelding herfra. Den
rekkefølgen bør avgjøres bevisst, ikke oppdages når renderen er ferdig.

Dokumentet kan altså bygges lenge før det kan leveres — akkurat som
RF-1086 (docs/aksjonaer.md). Forskjellen er at RF-1086 venter på et
scope som kan bestilles, mens dette venter på en annen
autentiseringsmodell.

## Artefaktene finnes, og de er gode

Skatteetaten publiserer XSD-er, kodelister og eksempler åpent i
[Skatteetaten/skattemeldingen](https://github.com/Skatteetaten/skattemeldingen)
— aktivt vedlikeholdt (daglige commits). Ingen innlogging, ingen
avtale. Det er samme situasjon som SAF-T og mva-melding: formatet er
vendorbart og kan valideres med xmllint i tester og CI.

Filene som gjelder oss:

```
src/resources/xsd/naeringsspesifikasjon_v*_ekstern.xsd
src/resources/xsd/skattemeldingUpersonlig_*                 (AS mfl.)
src/resources/kodeliste/<år>/                               (per inntektsår)
src/resources/eksempler/<år>/
```

### Den uavklarte antakelsen: hvilken versjon gjelder hvilket år

Verifisert fra Skatteetatens **egne eksempelfiler** i repoet:

| Inntektsår | Næringsspesifikasjon | Kilde |
| --- | --- | --- |
| 2021 | v2 | `eksempler/2021/Naeringspesifikasjon-AS-v2.xml` |
| 2022 | v3 | `eksempler/2022/Naeringspesifikasjon-AS-v3.xml` |
| 2023 | v4 | (utledet — ingen AS-eksempel i mappen) |
| 2024 | v5 | `eksempler/2024/upersonligSkattemeldingOgNaeringsspesifikasjonRequest.xml` (namespace `…:ekstern:v5`) |
| **2025** | **v6?** | **ikke verifisert** |
| **2026** | **v7?** | **ikke verifisert** |

Regelen `versjon = inntektsår − 2019` treffer alle tre verifiserte
punktene, og både v6 og v7 finnes i repoet med `generertDato` som
passer (v6: 2026-01-16, altså til leveringssesongen for 2025; v7:
2026-06-04). Kodelistene har mapper til og med 2026.

**Men det er en slutning, ikke en kilde.** Skatteetatens README er
utdatert (beskriver v2 som gjeldende), og siden «Oversikt over innhold
og struktur i skattemeldingen» oppgir ikke versjonsnumre. Å bygge på
slutningen ville brutt det #50 slo fast: årganger velges fra et
register med verifiserte oppføringer, og utenfor dekningen skal koden
si tydelig fra i stedet for å gjette.

**Neste skritt for den som tar saken:** spør brukerstøtte (samme
henvendelse som scope-bestillingen, docs/gov.md) hvilken
næringsspesifikasjonsversjon som gjelder inntektsår 2025 og 2026, og
skriv svaret inn i tabellen over med kilde. Det er én setning å spørre
om, og det er det eneste som mangler før registeret kan skrives.

## Det vi allerede kan fylle ut

Strukturen passer uvanlig godt, fordi feltene vi ville trengt er de vi
allerede beregner. Fra `naeringsspesifikasjon_v7_ekstern.xsd`:

```
Naeringsspesifikasjon
  partsreferanse            påkrevd
  inntektsaar               påkrevd
  resultatregnskap          VALGFRITT
  balanseregnskap           VALGFRITT
  spesifikasjonAvAnleggsmiddel   valgfritt
  beregnetNaeringsinntekt        valgfritt
  forskjellMellomRegnskapsmessigOgSkattemessigVerdi  valgfritt
  virksomhet                påkrevd
  … (resten valgfritt)
```

Et gyldig minimumsdokument er altså `partsreferanse` + `inntektsaar` +
`virksomhet` — og **resultatregnskap og balanseregnskap er valgfrie
elementer vi kan fylle helt ut i dag**:

| Element | Finnes allerede som |
| --- | --- |
| `resultatregnskap` | `regnmed-core::regnskap` — resultat gruppert per NS 4102, presentasjonsfortegn |
| `balanseregnskap` | samme modul, balanse med udisponert resultat |
| `spesifikasjonAvAnleggsmiddel` | `regnmed-db::asset` + `saldo_rapport` (saldogrupper a–j, sktl. §14-43) |
| `forskjellMellomRegnskapsmessigOgSkattemessigVerdi` | delvis: anleggsmidlenes midlertidige forskjeller beregnes alt i `saldo_rapport` |
| kontogruppering | `regnmed-core::saft` sin næringsspesifikasjon-kodeliste, allerede vendored per årgang (#50) |

Kodelisten SAF-T-eksporten bruker for gruppering **er** den samme
næringsspesifikasjonen (`docs/saft/naeringsspesifikasjon_2025-2026.csv`).
Broen fra hovedbok til skjema er med andre ord allerede bygget én gang.

## Anbefalt form når saken tas

Samme mønster som RF-1086, som allerede har vist seg:

1. Årgangsregister over (inntektsår → XSD-versjon) med **kilde per
   rad**, som `ARGANGER` i `regnmed-core::saft`. Utenfor dekningen:
   feil, ikke gjetning.
2. Håndrullet deterministisk renderer i `regnmed-core`, validert mot
   den vendorede XSD-en med xmllint i enhetstester **og** CI.
3. Forhåndsvisning som endepunkt + portal, med `leverbar: false` og en
   liste over hindringene — så dokumentet kan kontrolleres av et
   menneske lenge før det kan sendes.
4. Innsending sist, og først når ID-porten-veien er avklart (#26).

De spesialiserte delene (kraftverk, rederibeskatning, internprising,
finansforetak) er ikke i målgruppen og skal nektes høylytt, ikke fylles
ut med nuller.

## Årsregnskap til Regnskapsregisteret (#12)

Ikke undersøkt i samme runde. Merk at det er en **annen** innsending
til en **annen** mottaker (Regnskapsregisteret i BRREG, ikke
Skatteetaten), med egen frist og eget format — ikke en variant av
denne saken.
