//! E-postutsendelse (docs/faktura.md, #32):
//!
//! - POST /companies/{id}/invoices/{iid}/send                      send invoice PDF
//! - POST /companies/{id}/invoices/{iid}/reminders/{rid}/send      send purring PDF
//! - GET  /companies/{id}/invoices/{iid}/utsendelser               insert-only log
//!
//! Sending is always an explicit human action. Recipient defaults to
//! the party's stored e-mail (overridable per send); replies go to the
//! company's own address. Requires the mail rail (NATS_URL) — without
//! it the endpoints answer with a clear message instead of pretending.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::mailq::{OutboundMail, publish};

async fn require_write(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
) -> Result<(), ApiError> {
    let access = regnmed_db::company_access(&state.pool, person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if access == "les" {
        return Err(ApiError::Forbidden(
            "read-only access — utsendelse requires bokforing",
        ));
    }
    Ok(())
}

#[derive(Deserialize, Default)]
pub struct SendRequest {
    /// Overrides the party's stored e-mail for this send.
    email: Option<String>,
}

async fn send_payload(
    state: &AppState,
    person: &AuthPerson,
    company_id: Uuid,
    payload: regnmed_db::EmailPayload,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(js) = &state.mailq else {
        return Err(ApiError::BadRequest(
            "e-postutsendelse er ikke konfigurert (NATS_URL)".into(),
        ));
    };
    let id = Uuid::now_v7();
    let mail = OutboundMail::from_payload(id, &payload);
    publish(js, &mail)
        .await
        .map_err(|e| ApiError::BadRequest(format!("kunne ikke legge i utsendelseskøen: {e:#}")))?;
    let sent_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::log_utsendelse(
        &state.pool,
        id,
        company_id,
        payload.invoice_id,
        payload.reminder_id,
        &payload.to,
        &payload.subject,
        sent_by,
    )
    .await?;
    Ok(Json(json!({
        "utsendelse_id": id,
        "to": payload.to,
        "subject": payload.subject,
    })))
}

pub async fn send_invoice(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<SendRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_write(&state, person.person_id, company_id).await?;
    let request = body.map(|Json(r)| r).unwrap_or_default();
    let payload = regnmed_db::invoice_email_payload(
        &state.pool,
        company_id,
        invoice_id,
        request.email.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    send_payload(&state, &person, company_id, payload).await
}

pub async fn send_reminder(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id, reminder_id)): Path<(Uuid, Uuid, Uuid)>,
    body: Option<Json<SendRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_write(&state, person.person_id, company_id).await?;
    let request = body.map(|Json(r)| r).unwrap_or_default();
    let payload = regnmed_db::reminder_email_payload(
        &state.pool,
        company_id,
        invoice_id,
        reminder_id,
        request.email.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    send_payload(&state, &person, company_id, payload).await
}

pub async fn list_utsendelser(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let access = regnmed_db::company_access(&state.pool, person.person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let _ = access;
    let rows = regnmed_db::list_utsendelser(&state.pool, company_id, invoice_id).await?;
    Ok(Json(json!({
        "utsendelser": rows.iter().map(|u| json!({
            "utsendelse_id": u.id,
            "reminder_id": u.reminder_id,
            "to": u.to_email,
            "subject": u.subject,
            "sent_by": u.sent_by,
            "sent_at": u.sent_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}
