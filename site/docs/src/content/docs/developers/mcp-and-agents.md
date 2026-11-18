---
title: MCP og agenter
description: Ta med din egen LLM-agent. MCP-serveren, verktøyene, rolleprompter og oppgaver til en agent.
sidebar:
  order: 8
---

Kundens eget LLM, med kundens egne nøkler, kan arbeide som en ansatt eller en regnskapsfører i selskapet. regnmed hoster ingen modell og selger ingen tokens: kunden bringer agenten, regnmed stiller skinnen.

## Prinsippet: en agent er en bruker

Det finnes ingen egen agent-tilgangsmodell. En agent er en person med `kind = 'integrasjon'` som har fått et **rollenavn** i selskapet. Tilgangsoppslag, attribusjon og revisjonsspor er de samme som for et menneske, og tilbakekalling virker i samme øyeblikk. Det eneste en maskin aldri får, er en rettighet som styrer hvem som har tilgang. Se [Tilgang og roller](/developers/access-and-roles/).

Standarden er **forslagsrett, ikke bokføringsrett**. Gi agenten rollen `les` for å lese, eller `bokforing` når eieren vil la den føre. Slå på `krev_for_maskin` i attesteringspolicyen for at alt roboten fører skal ses av et menneske.

## MCP-serveren

`regnmed-mcp` er en tynn adapter foran API-et. Den har **ingen databasetilgang**, ingen egne nøkler og ingen egen tilgangsmodell. Den henter token med kundens `client_credentials` og glemmer det. Protokollen er JSON-RPC 2.0 over stdio (versjon `2025-06-18`) med `initialize`, `ping`, `tools/list`, `tools/call`, `prompts/list` og `prompts/get`.

Serveren kjører på kundens egen maskin ved siden av agenten. Du bygger den fra kildekoden:

```sh
cargo build --release -p regnmed-mcp
```

Ferdigbygde binærer per plattform i GitHub-releasene er ikke bygget ennå.

### Konfigurasjon

| Variabel | Betydning |
| --- | --- |
| `REGNMED_API` | API-ets basisadresse. Standard `https://api.regnmed.no` |
| `REGNMED_CLIENT_ID` | Klienten registrert i regnid |
| `REGNMED_CLIENT_SECRET` | Hemmeligheten til samme klient |
| `REGNMED_ISSUER` | IdP. Standard `https://id.regnmed.no`. Token-endepunktet oppdages |
| `REGNMED_TOKEN_URL` | Overstyrer oppdagelsen |
| `REGNMED_SCOPE` | Valgfritt scope på tokenforespørselen |
| `REGNMED_TOKEN` | Ferdig token i stedet for klientlegitimasjon (test) |
| `REGNMED_COMPANY` | Selskapet verktøyene arbeider for når et kall ikke navngir ett |

Mangler legitimasjonen, **starter ikke serveren**. En server som starter uten nøkler feiler først inne i et verktøykall, der agenten ikke kan skille konfigurasjonsfeil fra tilgangsfeil.

Eksempel i en MCP-klients konfigurasjon (for eksempel Claude Desktop):

```json
{
  "mcpServers": {
    "regnmed": {
      "command": "/sti/til/regnmed-mcp",
      "env": {
        "REGNMED_CLIENT_ID": "min-agent",
        "REGNMED_CLIENT_SECRET": "…",
        "REGNMED_COMPANY": "0192e8aa-…"
      }
    }
  }
}
```

Hemmeligheten står i agentens egen konfigurasjon og aldri i et repo.

## Verktøyene

Listen er **kuratert** (39 verktøy), ikke generert fra rutetabellen. En verktøyliste er en prompt, og to hundre verktøy er den sikreste måten å få en liten modell til å velge galt. En test kaller hvert verktøy mot det ekte API-et, så tabellen kan ikke gli stille fra det.

| Gruppe | Verktøy |
| --- | --- |
| Orientering | `selskaper` (kall først), `firmaopplysninger`, `kontoplan`, `periodestatus`, `attesteringspolicy` |
| Lese hovedboken | `bilag_liste`, `saldobalanse`, `kontospesifikasjon`, `resultat`, `balanse`, `mva_rapport`, `nokkeltall`, `kundespesifikasjon`, `leverandorspesifikasjon`, `revisjonsrapport` |
| Bilagsflyten | `innboks`, `innboks_forslag`, `bilag_last_opp`, `bilag_bokfor`, `bilag_for`, `bilag_avvis` |
| Salg | `kunder`, `fakturaer`, `forfalte_fakturaer`, `faktura_opprett` |
| Migrering | `saft_analyser`, `saft_importer`, `saft_logg`, `kontakter_importer`, `apne_poster_importer` |
| Utlegg | `utlegg_mottakere`, `utlegg_fra_innboks`, `utlegg_registrer`, `utlegg_liste`, `utlegg_kvittering`, `utlegg_forslag`, `utlegg_oppdater`, `utlegg_send_inn` |
| Oppgaver | `oppgaver`, `oppgave`, `oppgave_svar` |

Noen valg du bør kjenne til:

- **Filer går som sti**, ikke base64. `bilag_last_opp`, `saft_analyser`, `saft_importer` og CSV-importene tar en sti til en fil på agentens maskin, og serveren leser den selv. Binære opplastinger bærer `?sha256=`, så et nytt forsøk er en no-op.
- **Et bilde går som bilde.** `utlegg_kvittering` returnerer kvitteringen som MCP-bildeinnhold.
- **Feil er prosa.** Et feilet verktøykall kommer tilbake med `isError: true`, regnmeds egen melding og HTTP-statusen, fordi API-ets meldinger sier hva man skal gjøre. Protokollfeil (ukjent metode eller verktøy) er JSON-RPC-feil.
- **Beløp er heltall øre** (positivt er debet), **datoer er `YYYY-MM-DD`**, gjentatt i hvert skjema der det gjelder.

## Rolleprompter

Serveren leverer fire prompter over `prompts/get`: `regnskapsforer`, `bokforer`, `ansatt` og `migrering` (den siste er en oppgaveprompt, ikke en rolle). Hver er felles grunnregler pluss rollen, skrevet for en **liten modell**: imperativ, forbud først, fast arbeidsrekkefølge, verktøy navngitt presist, og «jeg vet ikke» har en konkret handling (`oppgave_svar` med status `sporsmal`). Innholdet er to doktriner: bilagstolkning foreslår og bokfører aldri, og attestering betyr at et menneske godkjenner.

Promptene er på norsk, fordi modellen skriver til norske brukere.

## Oppgaver til en agent

En agent får jobber som **oppgaver** i stedet for meldinger i en chat som forsvinner. Bestillingen er uforanderlig (en endret oppgave er en ny), sporet er insert-only og statusen beregnes.

| Status | Betyr |
| --- | --- |
| `ny` | Ingen hendelse ennå |
| `pagar` | Agenten har begynt |
| `sporsmal` | Agenten trenger en avgjørelse fra et menneske. **Ikke endelig**, alternativet er at den gjetter |
| `utfort` | Ferdig. **Endelig** |
| `avvist` | Avslått. **Endelig** |

`sporsmal` og `avvist` krever `notat`. Å **gi** en oppgave krever `INTEGRASJON_ADMIN`, som en maskin ikke kan ha, så en agent kan ikke skrive sine egne ordrer. Å **svare** krever å være agenten oppgaven er stilt til. Endepunkter: se [API: agenter](/developers/api/agents/).

## Agenten kan også hostes i portalen

Kunden kan opprette en agent direkte i portalen med navn, rolle, egne instrukser (opptil 4000 tegn) og egen LLM-leverandør (OpenAI-kompatibelt endepunkt eller OpenRouter). Portalen oppretter da klienten i regnid og tildelingen i ett steg (`POST /companies/{id}/agents`). Agenten har fortsatt sin egen identitet og går gjennom nøyaktig de samme vaktene.

## Bevisst utenfor

- Vi selger ingen tokens. Kunden bringer modellen og regningen.
- Ingen lagret agentbegrunnelse som egen flate i innboksen ennå.
- Ratebegrensningen er ikke hevet for agenter (standard 120 per minutt per integrasjon). Det finnes ikke et eget bulk-endepunkt, og en migrering av mange dokumenter kan treffe grensen.
- Ingen paginering lagt til i listeendepunktene.

Bakgrunn: [Ta med din egen agent](/teknisk/agent/).
