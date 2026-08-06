# Utkast: scope-bestilling via brukerstøtten (eksternjira.sits.no)

Sendes som ÉN henvendelse etter at brukerkonto er opprettet
(brukeradministrator først — egen kontotjeneste, ikke ID-porten).
Klient-id-en står i `~/.config/regnmed/maskinporten-test.env` på
maskinen som registrerte nøkkelen (docs/secrets.md). Slett gjerne
denne filen når bestillingen er sendt og gov.md er oppdatert med dato.

---

**Emne:** Bestilling av Maskinporten-scopes, sluttbrukersystem for
regnskap — test

**Virksomhet:** Bogentech AS, orgnr 935 115 086
**Miljø:** test (test.maskinporten.no)
**Maskinporten klient-id (test):** `<klient-id fra Samarbeidsportalen>`
**Kontaktperson:** André Biseth, tlf. 988 02 600

Vi utvikler regnskapssystemet regnmed (sluttbrukersystem) og ber om at
følgende scopes tildeles vår organisasjon i **testmiljøet**:

| Scope | Formål |
| --- | --- |
| `skatteetaten:mvameldingvalidering` | Validering av mva-melding før innsending (mva-meldingen bygges og valideres i systemet i dag) |
| `skatteetaten:innrapporteringamelding` | A-melding og avstemmingsrapport (lønnsmodul under utvikling) |
| `skatteetaten:skattekorttilarbeidsgiver` | Hente skattekort til forskuddstrekk i lønnskjøring |
| `skatteetaten:innrapporteringaksjonaerregisteroppgave` | Innsending av aksjonærregisteroppgaven (RF-1086-rendringen er ferdig og XSD-validert) |
| `skatteetaten:mvaregisteravgiftssubjekt` | Autoritativ sjekk av om et selskap er mva-registrert |
| `skatteetaten:frister` | Offisielle frister som kryssjekk mot systemets egne fristberegninger |

For **innsending** av mva-melding forstår vi det slik at det ikke
finnes noe `skatteetaten:`-scope, men at innsendingen går via Altinn 3
(`altinn:instances.read` / `altinn:instances.write`) mot
mva-melding-appen. Vi ber om at nødvendige tilganger på
Skatteetaten/Altinn-siden for dette settes opp samtidig, eventuelt at
dere bekrefter hva som kreves der.

For aksjonærregisteroppgaven er vi kjent med at det også kreves Altinn
systembruker; si gjerne fra om det er noe mer dere trenger fra oss der.

Produksjonstilgang bestilles separat når testintegrasjonen er
verifisert.

---

Husk (fra docs/gov.md): oppgi alltid orgnr, miljø og klient-id;
API og korrelasjons-id ved feilmeldinger i senere supporthenvendelser.
