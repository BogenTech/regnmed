---
title: "Integrations, agents and messages"
description: "Machine access, agents and tasks, conversations."
editUrl: false
sidebar:
  order: 9
---

Machine access, agents and tasks, conversations.

The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles](/en/developers/access-and-roles/)); "public" means no token, "—" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.

| Method | Path | Right |
| --- | --- | --- |
| GET | `/chat/contacts` | — |
| GET | `/chat/conversations` | — |
| POST | `/chat/conversations` | — |
| GET | `/chat/conversations/{conversation_id}` | — |
| POST | `/chat/conversations/{conversation_id}/messages` | — |
| POST | `/chat/conversations/{conversation_id}/read` | — |
| POST | `/chat/conversations/{conversation_id}/restart` | — |
| POST | `/chat/read-all` | — |
| GET | `/chat/stream` | — |
| GET | `/chat/unread` | — |
| GET | `/companies/{company_id}/agent/oppgaver` | `INTEGRASJON_LES` |
| POST | `/companies/{company_id}/agent/oppgaver` | `INTEGRASJON_ADMIN` |
| GET | `/companies/{company_id}/agent/oppgaver/{task_id}` | `INTEGRASJON_LES` |
| POST | `/companies/{company_id}/agent/oppgaver/{task_id}/svar` | `INTEGRASJON_ADMIN` |
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

## Shapes

Grant an integration access (`INTEGRASJON_ADMIN`):

```json
POST /companies/{id}/integrations
{ "client_id": "webshop", "navn": "The webshop", "kontakt": "it@shop.example", "access": "bokforing" }
```

Change the role: `PUT …/integrations/{iid}/rolle` with `{ "access": "les" }`. Revoke: `POST …/integrations/{iid}/revoke`. Mutating calls the integration has made: `GET …/integrations/log`.

Tasks for an agent:

```json
POST /companies/{id}/agent/oppgaver
{ "melding": "Post the receipts in the inbox", "integration_id": "…", "frist": "2026-12-01" }

POST /companies/{id}/agent/oppgaver/{oppgave_id}/svar     (only the agent the task is assigned to)
{ "status": "pagar" }   // pagar | sporsmal | utfort | avvist, with "notat" for sporsmal and avvist
```

See [MCP and agents](/en/developers/mcp-and-agents/).


## Further reading (Norwegian)

- [integrations](/teknisk/integrations/)
- [agent](/teknisk/agent/)
- [meldinger](/teknisk/meldinger/)
