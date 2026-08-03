//! Platform roles: systemadmin and support (docs/auth.md §8).
//!
//! The deliberately bounded exception to "no access path crosses a
//! company boundary". Everything a platform role can do lives under
//! `/platform/*` on its own sub-router; the company-scoped guard
//! (`tilgang::krev`) is untouched, and there is NO route from a platform
//! role into any company's ledger — no vouchers, no balances, no reports.
//!
//! The #57 safeguards are enforced structurally, not by convention:
//!
//! - **Logged**: the `vakt` middleware wraps the whole sub-router, so a
//!   `/platform` endpoint that forgets to log cannot exist. The log row
//!   is written BEFORE the handler runs — a refused call still shows the
//!   attempt — and synchronously: if the log insert fails, the call fails.
//! - **Visible**: `/companies/{id}/platform-access` (and the firm twin)
//!   serves the rows that concern that company to its own admins.
//! - **Time-limited**: `platform_member.valid_to` is NOT NULL; expiry and
//!   revocation are immediate (exclusive date, checked per request).
//!
//! No platform role, integration or token gets in here without an active
//! `platform_member` row; strangers get 404, like everywhere else.

use axum::extract::{Path, Query, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

/// What the middleware resolved: who is calling, and as what. Handlers
/// receive it as an `Extension` — its presence proves the call went
/// through the guard and was logged.
#[derive(Clone)]
pub struct PlattformKontekst {
    pub person_id: Uuid,
    pub rolle: String,
}

impl PlattformKontekst {
    fn er_systemadmin(&self) -> bool {
        self.rolle == "systemadmin"
    }

    /// Support may look and assign new memberships; everything else —
    /// customer registers, role changes, platform-user administration —
    /// is systemadmin territory.
    fn krev_systemadmin(&self) -> Result<(), ApiError> {
        if self.er_systemadmin() {
            Ok(())
        } else {
            Err(ApiError::Forbidden("SYSTEMADMIN"))
        }
    }
}

/// The `/platform` sub-router, merged into the main router. The state is
/// applied here so the middleware can carry it.
pub fn plattform_router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/platform/members", get(list_members).post(grant_member))
        .route("/platform/members/{member_id}", delete(end_member))
        .route("/platform/companies", get(list_companies))
        .route("/platform/firms", get(list_firms))
        .route("/platform/users", get(list_users))
        .route("/platform/customers", get(list_customers))
        .route(
            "/platform/users/{person_id}/companies/{company_id}",
            post(assign_company),
        )
        .route(
            "/platform/users/{person_id}/firms/{firm_id}",
            post(assign_firm),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, vakt))
}

/// Company/firm ids named in a `/platform/...` path, for the log row.
fn ider_i_sti(path: &str) -> (Option<Uuid>, Option<Uuid>) {
    let mut company = None;
    let mut firm = None;
    let mut segments = path.split('/').filter(|s| !s.is_empty()).peekable();
    while let Some(seg) = segments.next() {
        if let Some(next) = segments.peek() {
            match seg {
                "companies" => company = Uuid::parse_str(next).ok(),
                "firms" => firm = Uuid::parse_str(next).ok(),
                _ => {}
            }
        }
    }
    (company, firm)
}

/// The one seam every `/platform` call passes through: token → person →
/// active platform role → log row → handler. A person without an active
/// role gets 404 — whether `/platform` exists is not theirs to probe.
async fn vakt(State(state): State<AppState>, req: Request, next: Next) -> Response {
    use axum::extract::FromRequestParts;
    let (mut parts, body) = req.into_parts();
    let person = match AuthPerson::from_request_parts(&mut parts, &state).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    // Platform roles are held by humans. A machine token never gets one
    // (also refused at grant time) — refuse here too, so the invariant
    // does not depend on the database alone.
    if person.integration.is_some() {
        return ApiError::NotFound.into_response();
    }
    let rolle = match regnmed_db::active_platform_role(&state.pool, person.person_id).await {
        Ok(Some(r)) => r.rolle,
        Ok(None) => return ApiError::NotFound.into_response(),
        Err(e) => return ApiError::from(e).into_response(),
    };
    let path = parts.uri.path().to_string();
    let (company_id, firm_id) = ider_i_sti(&path);
    if let Err(e) = regnmed_db::log_platform_access(
        &state.pool,
        person.person_id,
        &rolle,
        parts.method.as_str(),
        &path,
        company_id,
        firm_id,
    )
    .await
    {
        // Synchronous and fatal on purpose: an unlogged platform call
        // must not happen, so the call does not happen.
        return ApiError::from(e).into_response();
    }
    parts.extensions.insert(PlattformKontekst {
        person_id: person.person_id,
        rolle,
    });
    next.run(Request::from_parts(parts, body)).await
}

#[derive(Deserialize)]
pub struct SokQuery {
    sok: Option<String>,
}

async fn list_members(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    let medlemmer = regnmed_db::list_platform_members(&state.pool).await?;
    Ok(Json(json!({
        "medlemmer": medlemmer.iter().map(|m| json!({
            "id": m.id,
            "person_id": m.person_id,
            "navn": m.navn,
            "epost": m.epost,
            "rolle": m.rolle,
            "valid_from": m.valid_from.to_string(),
            "valid_to": m.valid_to.to_string(),
            "notat": m.notat,
            "aktiv": m.aktiv,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct GrantRequest {
    epost: String,
    rolle: String,
    valid_to: NaiveDate,
    notat: String,
}

async fn grant_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Json(body): Json<GrantRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    let person_id = regnmed_db::person_by_email(&state.pool, &body.epost)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
        .ok_or_else(|| {
            ApiError::BadRequest(
                "ingen bruker med den adressen — personen må ha logget inn først".into(),
            )
        })?;
    let id = regnmed_db::grant_platform_role(
        &state.pool,
        person_id,
        &body.rolle,
        body.valid_to,
        &body.notat,
        Some(ctx.person_id),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "id": id })))
}

async fn end_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Path(member_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    regnmed_db::end_platform_member(&state.pool, member_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "avsluttet": true })))
}

async fn list_companies(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlattformKontekst>,
    Query(query): Query<SokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let selskaper = regnmed_db::platform_list_companies(&state.pool, query.sok.as_deref()).await?;
    Ok(Json(json!({
        "selskaper": selskaper.iter().map(|c| json!({
            "company_id": c.id,
            "orgnr": c.orgnr,
            "name": c.name,
        })).collect::<Vec<_>>(),
    })))
}

async fn list_firms(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlattformKontekst>,
    Query(query): Query<SokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let byraer = regnmed_db::platform_list_firms(&state.pool, query.sok.as_deref()).await?;
    Ok(Json(json!({
        "byraer": byraer.iter().map(|f| json!({
            "firm_id": f.id,
            "orgnr": f.orgnr,
            "name": f.name,
            "kind": f.kind,
        })).collect::<Vec<_>>(),
    })))
}

async fn list_users(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlattformKontekst>,
    Query(query): Query<SokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let brukere = regnmed_db::platform_list_users(&state.pool, query.sok.as_deref()).await?;
    Ok(Json(json!({
        "brukere": brukere.iter().map(|b| json!({
            "person_id": b.person_id,
            "navn": b.navn,
            "epost": b.epost,
            "kind": b.kind,
            "tilknytninger": b.tilknytninger.iter().map(|t| json!({
                "slag": t.slag,
                "id": t.id,
                "navn": t.navn,
                "orgnr": t.orgnr,
                "rolle": t.rolle,
                "aktiv": t.aktiv,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })))
}

/// Customer registers across companies — master data with the owning
/// company named, never balances. Systemadmin only: "it is only System
/// Admins that have access to ALL customers".
async fn list_customers(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Query(query): Query<SokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ctx.krev_systemadmin()?;
    let kunder = regnmed_db::platform_list_customers(&state.pool, query.sok.as_deref()).await?;
    Ok(Json(json!({
        "kunder": kunder.iter().map(|k| json!({
            "party_id": k.party_id,
            "party_no": k.party_no,
            "navn": k.navn,
            "orgnr": k.orgnr,
            "epost": k.epost,
            "selskap": {
                "company_id": k.company_id,
                "navn": k.company_navn,
                "orgnr": k.company_orgnr,
            },
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct AssignRequest {
    rolle: String,
}

async fn assign_company(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Path((person_id, company_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AssignRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    regnmed_db::platform_assign_company(
        &state.pool,
        company_id,
        person_id,
        &body.rolle,
        ctx.person_id,
        ctx.er_systemadmin(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "tildelt": true })))
}

async fn assign_firm(
    State(state): State<AppState>,
    Extension(ctx): Extension<PlattformKontekst>,
    Path((person_id, firm_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AssignRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    regnmed_db::platform_assign_firm(
        &state.pool,
        firm_id,
        person_id,
        &body.rolle,
        ctx.person_id,
        ctx.er_systemadmin(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "tildelt": true })))
}

fn innsyn_json(rader: &[regnmed_db::PlattformInnsyn]) -> serde_json::Value {
    json!({
        "innsyn": rader.iter().map(|r| json!({
            "navn": r.navn,
            "rolle": r.rolle,
            "method": r.method,
            "path": r.path,
            "tidspunkt": r.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })
}

/// What the platform did that concerns THIS company, read by the
/// company's own administrators. Lives on the main router behind the
/// ordinary company guard.
pub async fn company_platform_access(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    let rader = regnmed_db::platform_access_for_company(&state.pool, company_id).await?;
    Ok(Json(innsyn_json(&rader)))
}

/// The byrå twin, for firm admins.
pub async fn firm_platform_access(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(firm_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::byramedlemmer::require_firm_admin(&state, person.person_id, firm_id).await?;
    let rader = regnmed_db::platform_access_for_firm(&state.pool, firm_id).await?;
    Ok(Json(innsyn_json(&rader)))
}
