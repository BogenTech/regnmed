//! Machine access: who gets in, and what they have done
//! (docs/integrations.md, #45).
//!
//! - GET    /companies/{id}/integrations              grants + usage today
//! - POST   /companies/{id}/integrations              grant access (admin)
//! - POST   /companies/{id}/integrations/{iid}/revoke revoke (admin)
//! - GET    /companies/{id}/integrations/log          changing calls
//!
//! Granting and revoking require admin — that is letting a robot into the
//! company's books. Reading the list and the log requires only access: a
//! revisor must be able to see which machines have written.
//!
//! Since #101 the grant carries a ROLE NAME, the same vocabulary a
//! member gets, `admin` included when the owner decides it. The ceiling
//! is `Rett::kan_delegeres`, enforced where access is resolved
//! (`access::slaa_opp`) rather than by a shorter list of roles here: a
//! machine does the work, but never widens anyone's access.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, Role, require};
use crate::auth::{ApiError, AuthPerson};

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::IntegrationRead).await?;
    let rows = regnmed_db::list_integrations(&state.pool, company_id).await?;
    Ok(Json(json!({
        "integrasjoner": rows.iter().map(|g| json!({
            "grant_id": g.grant_id,
            "integration_id": g.integration_id,
            "client_id": g.client_id,
            "navn": g.name,
            "kontakt": g.kontakt,
            "access": g.access,
            "valid_from": g.valid_from.to_string(),
            "valid_to": g.valid_to.map(|d| d.to_string()),
            "aktiv": g.valid_to.is_none(),
            "created_by": g.created_by,
            "revoked_by": g.revoked_by,
            "rate_limit_min": g.rate_limit_min,
            "kall_i_dag": g.kall_i_dag,
            "avatar_farge": g.avatar_farge,
            "avatar_ikon": g.avatar_ikon,
            "system_prompt": g.system_prompt,
            "llm": g.llm.as_ref().map(|(lev, modell)| json!({
                "leverandor": lev, "modell": modell
            })),
        })).collect::<Vec<_>>(),
        // The face's colours are served from the code that validates
        // them, so the picker cannot offer one the server rejects.
        "farger": regnmed_db::AVATAR_FARGER,
        // Whether this installation can store a provider key at all
        // (docs/agent.md §8) — the portal offers the form only then.
        "llm_tilgjengelig": state.sealing_key.is_some(),
        // Whether this installation can create an agent's identity for
        // the customer (docs/agent.md §8.6). False means the portal has
        // to ask for a client_id, as it always did.
        "kan_opprette": state.regnid.is_some() && state.sealing_key.is_some(),
        "leverandorer": regnmed_db::agentllm::LEVERANDORER
            .iter()
            .map(|(name, url)| json!({ "navn": name, "base_url": url }))
            .collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct GrantRequest {
    /// The client_id the IdP issues tokens for — the token's `sub`.
    client_id: String,
    #[serde(rename = "navn")]
    name: String,
    kontakt: Option<String>,
    /// A role name, exactly as a member gets one: a built-in role or one
    /// of the company's own (#101). `revisor` is not assignable here any
    /// more than it is to a person — it follows a revisjonsoppdrag.
    access: String,
}

/// The roles an owner may hand a machine.
///
/// `revisor` is absent for the same reason it is absent for people: it
/// comes from an oppdrag, never from an assignment. `ukjent` is not a
/// role. Everything else is on the table — including `admin`, which is
/// the revision #101 made explicitly: the ceiling is
/// [`Rett::kan_delegeres`], enforced in the access lookup, not a
/// shorter list of roles here.
const TILDELBARE: [Role; 4] = [Role::Employee, Role::Read, Role::Bookkeeping, Role::Admin];

/// Refuses a role name that is neither built-in nor one of the
/// company's own ACTIVE roles. Without this a typo would be stored and
/// then resolve to `Rolle::Ukjent` — an integration with a grant and no
/// access, which looks like a bug in the API rather than in the request.
async fn krev_gyldig_rolle(state: &AppState, company_id: Uuid, name: &str) -> Result<(), ApiError> {
    let name = name.trim();
    if TILDELBARE.iter().any(|r| r.slug() == name) {
        return Ok(());
    }
    let egne = regnmed_db::roles::list_roller(&state.pool, company_id).await?;
    if egne.iter().any(|r| r.active && r.name == name) {
        return Ok(());
    }
    let valg: Vec<&str> = TILDELBARE
        .iter()
        .map(|r| r.slug())
        .chain(egne.iter().filter(|r| r.active).map(|r| r.name.as_str()))
        .collect();
    Err(ApiError::BadRequest(format!(
        "«{name}» er ingen rolle i dette selskapet — velg en av: {}",
        valg.join(", ")
    )))
}

pub async fn grant(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<GrantRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::IntegrationAdmin,
    )
    .await?;
    krev_gyldig_rolle(&state, company_id, &request.access).await?;
    let by = person.display().to_string();
    let id = regnmed_db::grant_integration(
        &state.pool,
        company_id,
        &request.client_id,
        &request.name,
        request.kontakt.as_deref(),
        &request.access,
        &by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({ "integration_id": id })))
}

#[derive(Deserialize)]
pub struct AgentRequest {
    #[serde(rename = "navn")]
    name: String,
    /// A role name, exactly as a member gets one.
    access: String,
    kontakt: Option<String>,
    avatar_farge: Option<String>,
    avatar_ikon: Option<String>,
    /// Extra instructions on top of the role prompt.
    system_prompt: Option<String>,
    /// An EXISTING client_id, for an agent the customer runs themselves
    /// (docs/agent.md §8, the first rail). Left out, the identity is
    /// created for them — which is the whole point of this endpoint.
    client_id: Option<String>,
    /// The model. Optional: an agent may be created now and given a
    /// provider later, and an agent on the customer's own machine never
    /// needs one here at all.
    #[serde(rename = "leverandor")]
    supplier: Option<String>,
    base_url: Option<String>,
    modell: Option<String>,
    api_nokkel: Option<String>,
}

/// Creates an agent in one call: identity, access, face, instructions
/// and model (docs/agent.md §8.6).
///
/// This is the same thing `grant` does, plus the two steps a customer
/// could not do for themselves — registering the OIDC client and
/// storing its secret. Nothing about the authorization model changes:
/// the agent is still a `person` with a grant, and the role on that
/// grant is still what every call it makes is checked against.
///
/// Order matters for what a half-finished run leaves behind. The IdP
/// client is created FIRST, because a machine client with no grant can
/// get a token and do nothing at all — it is inert. The reverse order
/// would leave the customer looking at an agent in their own list that
/// can never authenticate.
pub async fn opprett_agent(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<AgentRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::IntegrationAdmin,
    )
    .await?;
    krev_gyldig_rolle(&state, company_id, &request.access).await?;
    let by = person.display().to_string();

    let oppgitt = request
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty());

    // Provision only when no client_id was given. A customer bringing
    // their own agent keeps the identity they already have.
    let (client_id, hemmelighet) = match oppgitt {
        Some(client_id) => (client_id.to_string(), None),
        None => {
            let Some(cfg) = state.regnid.as_ref() else {
                return Err(ApiError::BadRequest(
                    "denne installasjonen kan ikke opprette agentidentiteter selv —                      oppgi en client_id fra påloggingstjenesten (docs/agent.md §8.6)"
                        .into(),
                ));
            };
            // The secret has to be stored sealed, so refuse before
            // creating anything we would then have to throw away.
            if state.sealing_key.is_none() {
                return Err(ApiError::BadRequest(
                    "vertsagenter er ikke satt opp på denne installasjonen                      (AGENT_SEALING_KEY mangler)"
                        .into(),
                ));
            }
            let client_id = format!("agent-{}", Uuid::now_v7().simple());
            let klient = regnmed_gov::regnid::Regnid::new(
                cfg.base_url
                    .as_deref()
                    .unwrap_or_else(|| state.verifier.issuer()),
                &cfg.client_id,
                &cfg.client_secret,
            )
            .create_machine_client(
                &client_id,
                request.name.trim(),
                state.verifier.audience().unwrap_or_default(),
            )
            .await
            .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
            (klient.client_id, Some(klient.client_secret))
        }
    };

    let integration_id = regnmed_db::grant_integration(
        &state.pool,
        company_id,
        &client_id,
        &request.name,
        request.kontakt.as_deref(),
        &request.access,
        &by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;

    // The face and the instructions are one update, and a failure here
    // is not worth losing the agent over: it is editable afterwards
    // from the same card, so say what went wrong and keep the agent.
    let mut advarsler: Vec<String> = Vec::new();
    let har_metadata = request.avatar_farge.is_some()
        || request.avatar_ikon.is_some()
        || request.system_prompt.is_some()
        || request.kontakt.is_some();
    if har_metadata
        && let Err(e) = regnmed_db::update_integration(
            &state.pool,
            company_id,
            integration_id,
            &request.name,
            request.kontakt.as_deref(),
            request.avatar_farge.as_deref(),
            request.avatar_ikon.as_deref(),
            request.system_prompt.as_deref(),
        )
        .await
    {
        advarsler.push(format!("agenten ble opprettet, men {e:#}"));
    }

    if let Some(hemmelighet) = hemmelighet {
        let nokkel = state
            .sealing_key
            .as_ref()
            .expect("sjekket før klienten ble opprettet");
        regnmed_db::agentllm::sett_klienthemmelighet(
            &state.pool,
            nokkel,
            company_id,
            integration_id,
            &hemmelighet,
        )
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    }

    // The model, when the form carried one. Partial model fields are a
    // mistake worth naming rather than storing half of.
    let modell = request
        .modell
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty());
    let api_nokkel = request
        .api_nokkel
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty());
    match (modell, api_nokkel) {
        (Some(modell), Some(api_nokkel)) => {
            let Some(nokkel) = state.sealing_key.as_ref() else {
                return Err(ApiError::BadRequest(
                    "vertsagenter er ikke satt opp på denne installasjonen                      (AGENT_SEALING_KEY mangler)"
                        .into(),
                ));
            };
            regnmed_db::agentllm::sett_llm(
                &state.pool,
                nokkel,
                company_id,
                integration_id,
                request.supplier.as_deref().unwrap_or("openai"),
                request.base_url.as_deref(),
                modell,
                api_nokkel,
                &by,
            )
            .await
            .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
        }
        (None, None) => {}
        _ => {
            advarsler.push(
                "agenten ble opprettet uten modell — både modell og API-nøkkel må oppgis".into(),
            );
        }
    }

    Ok(Json(json!({
        "integration_id": integration_id,
        "client_id": client_id,
        // Whether this agent can work on its own, or is waiting for
        // somebody to finish setting it up.
        "klar": modell.is_some() && api_nokkel.is_some(),
        "advarsler": advarsler,
    })))
}

#[derive(Deserialize)]
pub struct EndreRequest {
    #[serde(rename = "navn")]
    name: String,
    kontakt: Option<String>,
    avatar_farge: Option<String>,
    avatar_ikon: Option<String>,
    /// Extra instructions for this agent, on top of the role prompt
    /// (docs/agent.md §8.6). Empty clears them.
    system_prompt: Option<String>,
}

/// Rename an agent and give it a face (docs/agent.md §8.1). Takes
/// `IntegrasjonAdmin` like the grant itself: a name is what the trail
/// shows for everything the robot does, so changing it is administering
/// machine access, not decoration.
pub async fn endre(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, integration_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<EndreRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::IntegrationAdmin,
    )
    .await?;
    regnmed_db::update_integration(
        &state.pool,
        company_id,
        integration_id,
        &request.name,
        request.kontakt.as_deref(),
        request.avatar_farge.as_deref(),
        request.avatar_ikon.as_deref(),
        request.system_prompt.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({ "navn": request.name.trim() })))
}

#[derive(Deserialize)]
pub struct RolleRequest {
    access: String,
}

/// Changes an agent's role. One call, because that is how the user
/// thinks about it; underneath it is still revoke plus a new grant, so
/// the dated trail keeps the old role true for the time it applied.
pub async fn role(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, integration_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RolleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::IntegrationAdmin,
    )
    .await?;
    krev_gyldig_rolle(&state, company_id, &request.access).await?;
    regnmed_db::endre_integrasjonsrolle(
        &state.pool,
        company_id,
        integration_id,
        &request.access,
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({ "access": request.access.trim() })))
}

pub async fn revoke(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, integration_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::IntegrationAdmin,
    )
    .await?;
    let by = person.display().to_string();
    regnmed_db::revoke_integration(&state.pool, company_id, integration_id, &by)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "revoked": true })))
}

pub async fn log(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::IntegrationRead).await?;
    let rows = regnmed_db::integration_calls(&state.pool, company_id).await?;
    Ok(Json(json!({
        "kall": rows.iter().map(|c| json!({
            "navn": c.name,
            "method": c.method,
            "path": c.path,
            "status": c.status,
            "tidspunkt": c.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}

// ---- Vertsagent: kundens egen LLM-leverandør (docs/agent.md §8) ----

#[derive(Deserialize)]
pub struct LlmRequest {
    #[serde(rename = "leverandor")]
    supplier: String,
    base_url: Option<String>,
    modell: String,
    /// The customer's own key. Goes in sealed and never comes back out:
    /// [`llm`] reports whether one is set, never what it is.
    api_nokkel: String,
    /// The agent's own OIDC client secret (docs/agent.md §8.3). The
    /// worker needs it to get a token AS THE AGENT -- there is no
    /// second way into the ledger for it. Optional here so a key can be
    /// replaced without re-entering this one.
    klient_hemmelighet: Option<String>,
}

/// The provider this agent uses, and whether a key is stored. The key
/// itself has no route out of here — see docs/agent.md §8.
pub async fn llm(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, integration_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::IntegrationRead).await?;
    let oppsett = regnmed_db::agentllm::hent_llm(&state.pool, company_id, integration_id).await?;
    Ok(Json(json!({
        // Without a sealing key the rail is off, and the portal says so
        // rather than offering a form that would store a key we cannot
        // protect.
        "tilgjengelig": state.sealing_key.is_some(),
        "leverandorer": regnmed_db::agentllm::LEVERANDORER
            .iter()
            .map(|(name, url)| json!({ "navn": name, "base_url": url }))
            .collect::<Vec<_>>(),
        // Whether the worker can authenticate AS this agent at all.
        // Without it a model is configured but nothing can run it, and
        // the portal must be able to say which half is missing.
        "klient_lagret": regnmed_db::agentllm::har_klienthemmelighet(
            &state.pool, integration_id).await?,
        "oppsett": oppsett.filter(|o| o.active).map(|o| json!({
            "leverandor": o.supplier,
            "base_url": o.base_url,
            "modell": o.modell,
            "satt_av": o.satt_av,
            "satt_at": o.created_at.to_rfc3339(),
            "nokkel_lagret": true,
        })),
    })))
}

/// Records the provider and seals the key. Takes `IntegrasjonAdmin`:
/// this decides which company the ledger's contents are sent to, and
/// whose bill they are sent on.
pub async fn sett_llm(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, integration_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<LlmRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::IntegrationAdmin,
    )
    .await?;
    let Some(nokkel) = state.sealing_key.as_ref() else {
        return Err(ApiError::BadRequest(
            "vertsagenter er ikke satt opp på denne installasjonen (AGENT_SEALING_KEY mangler)"
                .into(),
        ));
    };
    regnmed_db::agentllm::sett_llm(
        &state.pool,
        nokkel,
        company_id,
        integration_id,
        &request.supplier,
        request.base_url.as_deref(),
        &request.modell,
        &request.api_nokkel,
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    if let Some(hemmelighet) = request.klient_hemmelighet.as_deref().map(str::trim)
        && !hemmelighet.is_empty()
    {
        regnmed_db::agentllm::sett_klienthemmelighet(
            &state.pool,
            nokkel,
            company_id,
            integration_id,
            hemmelighet,
        )
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    }
    Ok(Json(json!({ "leverandor": request.supplier })))
}

/// Turns the agent's model off. The row stays: which provider held this
/// company's data, and when, outlives the decision to stop using it.
pub async fn fjern_llm(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, integration_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::IntegrationAdmin,
    )
    .await?;
    regnmed_db::agentllm::fjern_llm(&state.pool, company_id, integration_id, person.display())
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({ "aktiv": false })))
}
