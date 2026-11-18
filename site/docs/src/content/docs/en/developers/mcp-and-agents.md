---
title: MCP and agents
description: Bring your own LLM agent. The MCP server, the tools, role prompts and tasks for an agent.
sidebar:
  order: 8
---

The customer's own LLM, with the customer's own keys, can work as an employee or an accountant in the company. regnmed hosts no model and sells no tokens: the customer brings the agent, regnmed provides the rails.

## The principle: an agent is a user

There is no separate agent access model. An agent is a person with `kind = 'integrasjon'` (integration) who has been given a **role name** in the company. Access lookup, attribution and audit trail are the same as for a human, and revocation takes effect at once. The one thing a machine never gets is a right that controls who has access. See [Access and roles](/en/developers/access-and-roles/).

The default is **right to propose, not right to post**. Give the agent the role `les` (read) to read, or `bokforing` (bookkeeping) when the owner wants it to post. Turn on `krev_for_maskin` (require for machine) in the approval policy so that everything the robot posts is seen by a human.

## The MCP server

`regnmed-mcp` is a thin adapter in front of the API. It has **no database access**, no keys of its own and no access model of its own. It fetches a token with the customer's `client_credentials` and forgets it. The protocol is JSON-RPC 2.0 over stdio (version `2025-06-18`) with `initialize`, `ping`, `tools/list`, `tools/call`, `prompts/list` and `prompts/get`.

The server runs on the customer's own machine next to the agent. You build it from source:

```sh
cargo build --release -p regnmed-mcp
```

Prebuilt per-platform binaries in the GitHub releases are not built yet.

### Configuration

| Variable | Meaning |
| --- | --- |
| `REGNMED_API` | The API's base URL. Default `https://api.regnmed.no` |
| `REGNMED_CLIENT_ID` | The client registered in regnid |
| `REGNMED_CLIENT_SECRET` | The secret for the same client |
| `REGNMED_ISSUER` | The IdP. Default `https://id.regnmed.no`. The token endpoint is discovered |
| `REGNMED_TOKEN_URL` | Overrides discovery |
| `REGNMED_SCOPE` | Optional scope on the token request |
| `REGNMED_TOKEN` | A ready-made token instead of client credentials (testing) |
| `REGNMED_COMPANY` | The company the tools work for when a call does not name one |

If the credentials are missing, **the server does not start**. A server that starts without keys fails only inside a tool call, where the agent cannot tell a configuration error from an access error.

Example in an MCP client's configuration (for example Claude Desktop):

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

The secret lives in the agent's own configuration and never in a repo.

## The tools

The list is **curated** (39 tools), not generated from the route table. A tool list is a prompt, and two hundred tools is the surest way to make a small model choose wrong. A test calls every tool against the real API, so the table cannot drift silently from it.

The tool names are Norwegian wire names and are kept as they are (for example `selskaper` = companies, `bilag_bokfor` = post voucher, `saldobalanse` = trial balance).

| Group | Tools |
| --- | --- |
| Orientation | `selskaper` (call first), `firmaopplysninger`, `kontoplan`, `periodestatus`, `attesteringspolicy` |
| Reading the general ledger | `bilag_liste`, `saldobalanse`, `kontospesifikasjon`, `resultat`, `balanse`, `mva_rapport`, `nokkeltall`, `kundespesifikasjon`, `leverandorspesifikasjon`, `revisjonsrapport` |
| Voucher flow | `innboks`, `innboks_forslag`, `bilag_last_opp`, `bilag_bokfor`, `bilag_for`, `bilag_avvis` |
| Sales | `kunder`, `fakturaer`, `forfalte_fakturaer`, `faktura_opprett` |
| Migration | `saft_analyser`, `saft_importer`, `saft_logg`, `kontakter_importer`, `apne_poster_importer` |
| Expenses | `utlegg_mottakere`, `utlegg_fra_innboks`, `utlegg_registrer`, `utlegg_liste`, `utlegg_kvittering`, `utlegg_forslag`, `utlegg_oppdater`, `utlegg_send_inn` |
| Tasks | `oppgaver`, `oppgave`, `oppgave_svar` |

A few choices you should know about:

- **Files go as a path**, not base64. `bilag_last_opp`, `saft_analyser`, `saft_importer` and the CSV imports take a path to a file on the agent's machine, and the server reads it itself. Binary uploads carry `?sha256=`, so a retry is a no-op.
- **An image goes as an image.** `utlegg_kvittering` returns the receipt as MCP image content.
- **Errors are prose.** A failed tool call comes back with `isError: true`, regnmed's own message and the HTTP status, because the API's messages say what to do. Protocol errors (unknown method or tool) are JSON-RPC errors.
- **Amounts are integer øre** (positive is debit), **dates are `YYYY-MM-DD`**, repeated in every schema where it applies.

## Role prompts

The server delivers four prompts over `prompts/get`: `regnskapsforer` (accountant), `bokforer` (bookkeeper), `ansatt` (employee) and `migrering` (migration; the last is a task prompt, not a role). Each is shared ground rules plus the role, written for a **small model**: imperative, prohibitions first, a fixed working order, tools named precisely, and "I don't know" has a concrete action (`oppgave_svar` with status `sporsmal`, question). The content is two doctrines: voucher interpretation proposes and never posts, and approval means a human approves.

The prompts are in Norwegian, because the model writes to Norwegian users.

## Tasks for an agent

An agent gets jobs as **tasks** instead of messages in a chat that disappears. The order is immutable (a changed task is a new one), the trail is insert-only and the status is computed.

| Status | Means |
| --- | --- |
| `ny` | No event yet (new) |
| `pagar` | The agent has started (in progress) |
| `sporsmal` | The agent needs a decision from a human. **Not final**, the alternative is that it guesses |
| `utfort` | Done. **Final** |
| `avvist` | Declined. **Final** |

`sporsmal` and `avvist` require `notat` (note). **Giving** a task requires `INTEGRASJON_ADMIN`, which a machine cannot have, so an agent cannot write its own orders. **Answering** requires being the agent the task is addressed to. Endpoints: see [API: agents](/en/developers/api/agents/).

## The agent can also be hosted in the portal

The customer can create an agent directly in the portal with a name, role, its own instructions (up to 4000 characters) and its own LLM provider (OpenAI-compatible endpoint or OpenRouter). The portal then creates the client in regnid and the grant in one step (`POST /companies/{id}/agents`). The agent still has its own identity and goes through exactly the same guards.

## Deliberately out of scope

- We sell no tokens. The customer brings the model and the bill.
- No stored agent reasoning as a separate surface in the inbox yet.
- The rate limit is not raised for agents (default 120 per minute per integration). There is no separate bulk endpoint, and a migration of many documents can hit the limit.
- No pagination added to the list endpoints.

Background: [Bring your own agent](/teknisk/agent/).
