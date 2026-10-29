//! The customer's own LLM provider, per agent (docs/agent.md §8).
//!
//! Two rules decide the shape of everything here:
//!
//! 1. **The key is never read back out through the API.** It goes in
//!    sealed and comes out only where it is used. What a caller may see
//!    is the provider, the model and whether a key is set at all.
//! 2. **The provider is the customer's choice and part of the record.**
//!    Sending ledger lines to a model provider makes that provider a
//!    sub-processor of the customer's accounting data, so rows are
//!    insert-only and the newest one governs — the trail says which
//!    provider was chosen, by whom, and from when.

use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use regnmed_core::hemmelig::{Nokkel, apne, forsegl};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// What a caller may see. Deliberately has no field for the key.
#[derive(Debug)]
pub struct LlmOppsett {
    pub leverandor: String,
    pub base_url: String,
    pub modell: String,
    pub aktiv: bool,
    pub satt_av: String,
    pub created_at: DateTime<Utc>,
}

/// The providers the portal offers, with the base URL each one implies.
/// `annet` is here so a customer can point at their own gateway without
/// us shipping a release for it — the URL is then theirs to give.
pub const LEVERANDORER: [(&str, &str); 3] = [
    ("openai", "https://api.openai.com/v1"),
    ("openrouter", "https://openrouter.ai/api/v1"),
    ("annet", ""),
];

fn standard_url(leverandor: &str) -> Option<&'static str> {
    LEVERANDORER
        .iter()
        .find(|(navn, _)| *navn == leverandor)
        .map(|(_, url)| *url)
}

/// Records the provider for this agent in this company, sealing the key
/// on the way in. Replacing the configuration is a new row.
pub async fn sett_llm(
    pool: &PgPool,
    nokkel: &Nokkel,
    company_id: Uuid,
    integration_id: Uuid,
    leverandor: &str,
    base_url: Option<&str>,
    modell: &str,
    api_nokkel: &str,
    satt_av: &str,
) -> Result<()> {
    let leverandor = leverandor.trim();
    let modell = modell.trim();
    let api_nokkel = api_nokkel.trim();
    let standard = standard_url(leverandor);
    ensure!(
        standard.is_some(),
        "ukjent leverandør «{leverandor}» — gyldige er {}",
        LEVERANDORER
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(", ")
    );
    ensure!(!modell.is_empty(), "modellen mangler");
    ensure!(!api_nokkel.is_empty(), "API-nøkkelen mangler");

    // A given base_url wins, so `annet` works and a customer can point a
    // known provider at a proxy; otherwise the provider's own.
    let url = base_url
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| standard.unwrap_or(""));
    ensure!(
        !url.is_empty(),
        "leverandøren «{leverandor}» trenger en adresse (base_url)"
    );
    ensure!(
        url.starts_with("https://"),
        "adressen må være https — kundens nøkkel og hovedbok går over den"
    );

    // The agent must actually belong to this company. Without this an
    // admin could configure, and pay for, an integration they merely
    // know the id of.
    let knyttet: bool = sqlx::query_scalar(
        "select exists (select 1 from integration_grant
                        where integration_id = $1 and company_id = $2)",
    )
    .bind(integration_id)
    .bind(company_id)
    .fetch_one(pool)
    .await?;
    ensure!(knyttet, "ingen slik integrasjon på dette selskapet");

    sqlx::query(
        "insert into agent_llm (id, integration_id, company_id, leverandor, base_url,
                                modell, nokkel, aktiv, satt_av)
         values ($1, $2, $3, $4, $5, $6, $7, true, $8)",
    )
    .bind(Uuid::now_v7())
    .bind(integration_id)
    .bind(company_id)
    .bind(leverandor)
    .bind(url)
    .bind(modell)
    .bind(forsegl(nokkel, api_nokkel.as_bytes())?)
    .bind(satt_av)
    .execute(pool)
    .await?;
    Ok(())
}

/// Turns the agent's model off without losing the record of what it was.
pub async fn fjern_llm(
    pool: &PgPool,
    company_id: Uuid,
    integration_id: Uuid,
    satt_av: &str,
) -> Result<()> {
    let forrige = hent_llm(pool, company_id, integration_id).await?;
    let forrige = forrige.filter(|o| o.aktiv);
    let Some(forrige) = forrige else {
        anyhow::bail!("agenten har ingen aktiv modell å slå av");
    };
    sqlx::query(
        "insert into agent_llm (id, integration_id, company_id, leverandor, base_url,
                                modell, nokkel, aktiv, satt_av)
         values ($1, $2, $3, $4, $5, $6, null, false, $7)",
    )
    .bind(Uuid::now_v7())
    .bind(integration_id)
    .bind(company_id)
    .bind(forrige.leverandor)
    .bind(forrige.base_url)
    .bind(forrige.modell)
    .bind(satt_av)
    .execute(pool)
    .await?;
    Ok(())
}

const NYESTE: &str = "select leverandor, base_url, modell, aktiv, satt_av, created_at
                      from agent_llm
                      where integration_id = $1 and company_id = $2
                      order by created_at desc, id desc limit 1";

/// The governing configuration, without the key.
pub async fn hent_llm(
    pool: &PgPool,
    company_id: Uuid,
    integration_id: Uuid,
) -> Result<Option<LlmOppsett>> {
    let row = sqlx::query(NYESTE)
        .bind(integration_id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| LlmOppsett {
        leverandor: r.get("leverandor"),
        base_url: r.get("base_url"),
        modell: r.get("modell"),
        aktiv: r.get("aktiv"),
        satt_av: r.get("satt_av"),
        created_at: r.get("created_at"),
    }))
}

/// The key, opened — for the worker that is about to call the provider
/// (docs/agent.md §8, step 3). Separate from [`hent_llm`] so that
/// reading the configuration for display cannot accidentally be the
/// call that decrypts.
pub async fn hent_llm_nokkel(
    pool: &PgPool,
    nokkel: &Nokkel,
    company_id: Uuid,
    integration_id: Uuid,
) -> Result<Option<(LlmOppsett, String)>> {
    let row = sqlx::query(
        "select leverandor, base_url, modell, aktiv, satt_av, created_at, nokkel
         from agent_llm
         where integration_id = $1 and company_id = $2
         order by created_at desc, id desc limit 1",
    )
    .bind(integration_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let aktiv: bool = row.get("aktiv");
    if !aktiv {
        return Ok(None);
    }
    let sealed: Vec<u8> = row.get::<Option<Vec<u8>>, _>("nokkel").unwrap_or_default();
    let klar = String::from_utf8(apne(nokkel, &sealed)?)?;
    Ok(Some((
        LlmOppsett {
            leverandor: row.get("leverandor"),
            base_url: row.get("base_url"),
            modell: row.get("modell"),
            aktiv,
            satt_av: row.get("satt_av"),
            created_at: row.get("created_at"),
        },
        klar,
    )))
}

// ---- Agentens egen klienthemmelighet (docs/agent.md §8.3) ----

/// Stores the agent's OIDC client secret, sealed. The client_id is the
/// integration's `oidc_sub` and is already here; this is the other half,
/// and it exists so the worker can get a token as the agent itself
/// rather than acting on its behalf through some second door.
pub async fn sett_klienthemmelighet(
    pool: &PgPool,
    nokkel: &Nokkel,
    company_id: Uuid,
    integration_id: Uuid,
    hemmelighet: &str,
) -> Result<()> {
    let hemmelighet = hemmelighet.trim();
    ensure!(!hemmelighet.is_empty(), "klienthemmeligheten mangler");
    let oppdatert = sqlx::query(
        "update integration set klient_hemmelighet = $3
         where id = $1
           and exists (select 1 from integration_grant g
                       where g.integration_id = $1 and g.company_id = $2)",
    )
    .bind(integration_id)
    .bind(company_id)
    .bind(forsegl(nokkel, hemmelighet.as_bytes())?)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(oppdatert == 1, "ingen slik integrasjon på dette selskapet");
    Ok(())
}

/// The agent's client_id and secret, opened — for the worker that is
/// about to ask the IdP for a token as this agent.
pub async fn hent_klientlegitimasjon(
    pool: &PgPool,
    nokkel: &Nokkel,
    integration_id: Uuid,
) -> Result<Option<(String, String)>> {
    let row = sqlx::query(
        "select p.oidc_sub, i.klient_hemmelighet
         from integration i join person p on p.id = i.person_id
         where i.id = $1",
    )
    .bind(integration_id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let Some(sealed) = row.get::<Option<Vec<u8>>, _>("klient_hemmelighet") else {
        return Ok(None);
    };
    Ok(Some((
        row.get("oidc_sub"),
        String::from_utf8(apne(nokkel, &sealed)?)?,
    )))
}

/// Whether a secret is stored, for display. Never the value.
pub async fn har_klienthemmelighet(pool: &PgPool, integration_id: Uuid) -> Result<bool> {
    Ok(
        sqlx::query_scalar("select klient_hemmelighet is not null from integration where id = $1")
            .bind(integration_id)
            .fetch_optional(pool)
            .await?
            .unwrap_or(false),
    )
}

/// Open tasks that a hosted agent can actually pick up: addressed to an
/// integration, not yet finished, and that integration has an active
/// model. Tasks for an agent on the customer's own machine are left
/// alone — that agent fetches its own queue.
pub async fn apne_agentoppgaver(
    pool: &PgPool,
    company_id: Option<Uuid>,
) -> Result<Vec<(Uuid, Uuid)>> {
    let rows = sqlx::query(
        "select o.company_id, o.id
         from agent_oppgave o
         join integration i on i.id = o.integration_id
         where ($1::uuid is null or o.company_id = $1)
           and coalesce((select h.status from agent_oppgave_hendelse h
                         where h.oppgave_id = o.id
                         order by h.created_at desc, h.id desc limit 1), 'ny')
               not in ('utfort', 'avvist', 'sporsmal')
           and exists (select 1 from agent_llm l
                       where l.integration_id = i.id and l.company_id = o.company_id
                         and l.aktiv
                         and l.created_at = (select max(created_at) from agent_llm l2
                                             where l2.integration_id = l.integration_id
                                               and l2.company_id = l.company_id))
         order by o.created_at",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}
