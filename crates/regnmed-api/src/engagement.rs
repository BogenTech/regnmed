//! Directory and oppdrag endpoints:
//!
//! - GET  /directory/firms?kind=regnskap        verified firms only
//! - GET  /firms/mine                           my firms + pending counts
//! - GET  /firms/{fid}/requests                 (firm member)
//! - POST /firms/{fid}/requests/{rid}/decision  {accept} (firm member)
//! - GET  /firms/{fid}/clients                  (firm member)
//! - GET  /companies/{id}/engagements           (any company access)
//! - POST /companies/{id}/engagement-requests   {firm_id, message?} (admin)
//! - POST /companies/{id}/engagements/{eid}/end (admin)

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};

async fn require_company_admin(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
) -> Result<(), ApiError> {
    let access = regnmed_db::company_access(&state.pool, person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if access != "admin" {
        return Err(ApiError::Forbidden("krever admin-tilgang i selskapet"));
    }
    Ok(())
}

async fn require_firm_member(
    state: &AppState,
    person_id: Uuid,
    firm_id: Uuid,
) -> Result<(), ApiError> {
    if !regnmed_db::is_firm_member(&state.pool, person_id, firm_id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct DirectoryQuery {
    kind: Option<String>,
}

pub async fn directory(
    State(state): State<AppState>,
    _person: AuthPerson,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let firms = regnmed_db::list_verified_firms(&state.pool, query.kind.as_deref()).await?;
    Ok(Json(json!({
        "firms": firms.iter().map(|f| json!({
            "firm_id": f.firm_id,
            "orgnr": f.orgnr,
            "name": f.name,
            "kind": f.kind,
            "client_count": f.client_count,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn my_firms(
    State(state): State<AppState>,
    person: AuthPerson,
) -> Result<Json<serde_json::Value>, ApiError> {
    let firms = regnmed_db::my_firms(&state.pool, person.person_id).await?;
    Ok(Json(json!({
        "firms": firms.iter().map(|f| json!({
            "firm_id": f.firm_id,
            "name": f.name,
            "kind": f.kind,
            "verified": f.verified,
            "pending_requests": f.pending_requests,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn firm_requests(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(firm_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_firm_member(&state, person.person_id, firm_id).await?;
    let requests = regnmed_db::firm_requests(&state.pool, firm_id).await?;
    Ok(Json(json!({
        "requests": requests.iter().map(|r| json!({
            "request_id": r.request_id,
            "company": r.company_name,
            "orgnr": r.company_orgnr,
            "kind": r.kind,
            "message": r.message,
            "status": r.status,
            "created_at": r.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct DecisionRequest {
    accept: bool,
}

pub async fn decide_request(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((firm_id, request_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<DecisionRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_firm_member(&state, person.person_id, firm_id).await?;
    regnmed_db::decide_request(
        &state.pool,
        firm_id,
        request_id,
        person.person_id,
        request.accept,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "accepted": request.accept })))
}

pub async fn firm_clients(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(firm_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_firm_member(&state, person.person_id, firm_id).await?;
    let clients = regnmed_db::firm_clients(&state.pool, firm_id).await?;
    Ok(Json(json!({ "clients": engagement_rows(&clients) })))
}

pub async fn company_engagements(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    regnmed_db::company_access(&state.pool, person.person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let engagements = regnmed_db::company_engagements(&state.pool, company_id).await?;
    Ok(Json(
        json!({ "engagements": engagement_rows(&engagements) }),
    ))
}

fn engagement_rows(rows: &[regnmed_db::EngagementRow]) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|e| {
            json!({
                "engagement_id": e.engagement_id,
                "firm_id": e.firm_id,
                "firm": e.firm_name,
                "company_id": e.company_id,
                "company": e.company_name,
                "kind": e.kind,
                "valid_from": e.valid_from.to_string(),
                "valid_to": e.valid_to.map(|d| d.to_string()),
            })
        })
        .collect()
}

#[derive(Deserialize)]
pub struct EngagementRequestBody {
    firm_id: Uuid,
    message: Option<String>,
}

pub async fn request_engagement(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<EngagementRequestBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_company_admin(&state, person.person_id, company_id).await?;
    let request_id = regnmed_db::request_engagement(
        &state.pool,
        company_id,
        request.firm_id,
        request.message.as_deref(),
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "request_id": request_id })))
}

pub async fn end_engagement(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, engagement_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_company_admin(&state, person.person_id, company_id).await?;
    regnmed_db::end_engagement(&state.pool, engagement_id, Some(company_id), None)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ended": true })))
}
