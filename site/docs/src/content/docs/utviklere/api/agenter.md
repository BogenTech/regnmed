---
title: "Integrasjoner, agenter og meldinger"
description: "Maskintilgang, agenter og oppgaver, samtaler."
sidebar:
  order: 9
---

Maskintilgang, agenter og oppgaver, samtaler.

Tabellen er generert fra rutetabellen i `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Rett** er rettigheten handleren krever (se [Tilgang og roller](/utviklere/tilgang-og-roller/)); «åpen» betyr uten token, «—» at kontrollen ligger i en hjelpefunksjon eller at bare innlogging kreves. Der flere retter står, avhenger kravet av parametrene.

| Metode | Sti | Rett |
| --- | --- | --- |
| GET | `/chat/contacts` | — |
| GET | `/chat/conversations` | — |
| POST | `/chat/conversations` | — |
| GET | `/chat/conversations/{conversation_id}` | — |
| POST | `/chat/conversations/{conversation_id}/messages` | — |
| POST | `/chat/conversations/{conversation_id}/read` | — |
| GET | `/chat/stream` | — |
| GET | `/chat/unread` | — |
| GET | `/companies/{company_id}/agent/oppgaver` | `INTEGRASJON_LES` |
| POST | `/companies/{company_id}/agent/oppgaver` | `INTEGRASJON_ADMIN` |
| GET | `/companies/{company_id}/agent/oppgaver/{task_id}` | `INTEGRASJON_LES` |
| POST | `/companies/{company_id}/agent/oppgaver/{task_id}/svar` | `INTEGRASJON_LES`, `INTEGRASJON_ADMIN` |
| POST | `/companies/{company_id}/agents` | `INTEGRASJON_ADMIN` |
| GET | `/companies/{company_id}/integrations` | `INTEGRASJON_LES` |
| POST | `/companies/{company_id}/integrations` | `INTEGRASJON_ADMIN` |
| GET | `/companies/{company_id}/integrations/log` | `INTEGRASJON_LES` |
| PUT | `/companies/{company_id}/integrations/{integration_id}` | `INTEGRASJON_ADMIN` |
| DELETE | `/companies/{company_id}/integrations/{integration_id}/llm` | `INTEGRASJON_ADMIN` |
| GET | `/companies/{company_id}/integrations/{integration_id}/llm` | `INTEGRASJON_LES` |
| PUT | `/companies/{company_id}/integrations/{integration_id}/llm` | `INTEGRASJON_ADMIN` |
| POST | `/companies/{company_id}/integrations/{integration_id}/revoke` | `INTEGRASJON_ADMIN` |
| PUT | `/companies/{company_id}/integrations/{integration_id}/rolle` | `INTEGRASJON_ADMIN` |

## Former

Gi en integrasjon tilgang (`INTEGRASJON_ADMIN`):

```json
POST /companies/{id}/integrations
{ "client_id": "nettbutikk", "navn": "Nettbutikken", "kontakt": "it@butikk.example", "access": "bokforing" }
```

Bytt rolle: `PUT …/integrations/{iid}/rolle` med `{ "access": "les" }`. Trekk tilbake: `POST …/integrations/{iid}/revoke`. Endrende kall integrasjonen har gjort: `GET …/integrations/log`.

Oppgaver til en agent:

```json
POST /companies/{id}/agent/oppgaver
{ "melding": "Bokfør kvitteringene i innboksen", "integration_id": "…", "frist": "2026-12-01" }

POST /companies/{id}/agent/oppgaver/{oppgave_id}/svar     (bare agenten oppgaven er stilt til)
{ "status": "pagar" }   // pagar | sporsmal | utfort | avvist, med "notat" ved sporsmal og avvist
```

Se [MCP og agenter](/utviklere/mcp-og-agenter/).


## Les mer

- [integrations](/teknisk/integrations/)
- [agent](/teknisk/agent/)
- [meldinger](/teknisk/meldinger/)
