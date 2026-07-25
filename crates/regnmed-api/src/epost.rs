//! E-post-inn: adresse, avsenderliste og karantene
//! (docs/epost-inn.md, #35).
//!
//! - GET    /companies/{id}/inbox/settings                 adresse + avsenderliste
//! - POST   /companies/{id}/inbox/settings/address         ny adresse (roterer, admin)
//! - POST   /companies/{id}/inbox/settings/senders         {sender, note} (admin)
//! - DELETE /companies/{id}/inbox/settings/senders/{sid}   fjern (admin)
//! - GET    /companies/{id}/inbox/mail[?status=karantene]  mottakslogg
//! - POST   /companies/{id}/inbox/mail/{mid}/release       slipp gjennom (admin)
//! - POST   /companies/{id}/inbox/mail/{mid}/reject        {note} (admin)
//!
//! Loggen er lesbar for alle med tilgang — en revisor skal kunne se hva
//! som kom inn og hva som ble gjort med det. Å endre hvem som får
//! levere, og å avgjøre karantene, krever admin.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};

async fn require_access(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
    admin: bool,
) -> Result<(), ApiError> {
    let access = regnmed_db::company_access(&state.pool, person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if admin && access != "admin" {
        return Err(ApiError::Forbidden("krever admin"));
    }
    Ok(())
}

/// The domain inbound addresses live under. Without it configured we
/// say so instead of printing an address that cannot receive anything.
fn mail_domain() -> Option<String> {
    std::env::var("MAIL_IN_DOMAIN")
        .ok()
        .filter(|d| !d.is_empty())
}

fn full_address(local: &str) -> serde_json::Value {
    match mail_domain() {
        Some(domain) => json!(format!("{local}@{domain}")),
        None => serde_json::Value::Null,
    }
}

pub async fn settings(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let local = regnmed_db::mail_address(&state.pool, company_id).await?;
    let senders = regnmed_db::list_allowed_senders(&state.pool, company_id).await?;
    Ok(Json(json!({
        // Without a mail rail there is no reception, and the portal says
        // that rather than showing a dead address.
        "aktiv": state.mailq.is_some() && mail_domain().is_some(),
        "domene": mail_domain(),
        "local_part": local,
        "adresse": local.as_deref().map(full_address),
        "avsendere": senders.iter().map(|s| json!({
            "id": s.id,
            "sender": s.sender,
            "note": s.note,
            "created_by": s.created_by,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn rotate_address(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let by = person.name.as_deref().unwrap_or(&person.sub);
    let local = regnmed_db::rotate_mail_address(&state.pool, company_id, by)
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({
        "local_part": local,
        "adresse": full_address(&local),
    })))
}

#[derive(Deserialize)]
pub struct SenderRequest {
    sender: String,
    note: Option<String>,
}

pub async fn add_sender(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<SenderRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::allow_sender(
        &state.pool,
        company_id,
        &request.sender,
        request.note.as_deref(),
        by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove_sender(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, sender_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    regnmed_db::revoke_sender(&state.pool, company_id, sender_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct MailQuery {
    status: Option<String>,
}

pub async fn list_mail(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<MailQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let rows = regnmed_db::list_mail(&state.pool, company_id, query.status.as_deref()).await?;
    Ok(Json(json!({
        "mail": rows.iter().map(|m| json!({
            "mail_id": m.id,
            "message_id": m.message_id,
            "fra": m.from_address,
            "emne": m.subject,
            "tekst": m.body,
            "antall_vedlegg": m.antall_vedlegg,
            "mottatt": m.received_at.to_rfc3339(),
            "status": m.status,
            "note": m.note,
            "decided_by": m.decided_by,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize, Default)]
pub struct ReleaseRequest {
    /// Also add the sender to the allow-list, so the next mail from
    /// them goes straight through.
    tillat_avsender: Option<bool>,
}

pub async fn release(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, mail_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<ReleaseRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let request = body.map(|Json(r)| r).unwrap_or_default();
    let by = person.name.as_deref().unwrap_or(&person.sub);
    let antall = regnmed_db::release_mail(
        &state.pool,
        company_id,
        mail_id,
        request.tillat_avsender.unwrap_or(false),
        by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({ "dokumenter": antall })))
}

#[derive(Deserialize)]
pub struct RejectRequest {
    note: String,
}

pub async fn reject(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, mail_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RejectRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::reject_mail(&state.pool, company_id, mail_id, &request.note, by)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "avvist": true })))
}
