//! E-postutsendelse (docs/faktura.md, #32):
//!
//! - POST /companies/{id}/invoices/{iid}/send                      queue the invoice (202)
//! - POST /companies/{id}/invoices/{iid}/delivery-check            queue a channel lookup (202)
//! - POST /companies/{id}/invoices/{iid}/reminders/{rid}/send      send purring PDF
//! - POST /companies/{id}/invitations/{iid}/resend                 send the invitation again
//! - GET  /companies/{id}/invoices/{iid}/utsendelser               insert-only log
//!
//! An invoice goes out from `regnmed work`, on the channel the worker
//! chooses (docs/efaktura.md §3.1): this API never calls the provider.
//! Sending is always an explicit human action. Recipient defaults to
//! the party's stored e-mail (overridable per send); replies go to the
//! company's own address. Requires the mail rail (NATS_URL) — without
//! it the endpoints answer with a clear message instead of pretending.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use regnmed_db::job::{self, Kind};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};
use crate::mailq::{OutboundMail, publish};

#[derive(Deserialize, Default)]
pub struct SendRequest {
    /// Overrides the party's stored e-mail for this send.
    email: Option<String>,
    /// A human's explicit resend of an unresolved Peppol attempt.
    #[serde(default)]
    resend_unresolved: bool,
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
    regnmed_db::log_dispatch(
        &state.pool,
        id,
        company_id,
        payload.invoice_id,
        payload.reminder_id,
        payload.invitation_id,
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
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    require(&state, person.person_id, company_id, Right::InvoiceSend).await?;
    if state.mailq.is_none() {
        return Err(ApiError::BadRequest(
            "e-postutsendelse er ikke konfigurert (NATS_URL)".into(),
        ));
    }
    let request = body.map(|Json(r)| r).unwrap_or_default();
    regnmed_db::peppol_worker::check_sendable(
        &state.pool,
        company_id,
        invoice_id,
        request.email.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let job_id = job::create_deduped(
        &state.pool,
        company_id,
        Kind::InvoiceDispatch,
        &invoice_id.to_string(),
        &json!({
            "invoice_id": invoice_id,
            "email": request.email,
            "resend_unresolved": request.resend_unresolved,
        }),
        person.name.as_deref().unwrap_or(&person.sub),
        Some(person.person_id),
    )
    .await?;
    crate::jobs::enqueue(&state, job_id).await;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "job_id": job_id, "status": "queued" })),
    ))
}

/// Which channel the invoice would go out on. The answer is the job's
/// result, because finding out takes a call to the provider.
pub async fn delivery_check(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    require(&state, person.person_id, company_id, Right::InvoiceSend).await?;
    // Without a queue the job would run here, and the API makes no lookups.
    if state.mailq.is_none() {
        return Err(ApiError::BadRequest(
            "utsendelse er ikke konfigurert (NATS_URL)".into(),
        ));
    }
    regnmed_db::peppol_worker::check_invoice(&state.pool, company_id, invoice_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let job_id = job::create_deduped(
        &state.pool,
        company_id,
        Kind::PeppolLookup,
        &invoice_id.to_string(),
        &json!({ "invoice_id": invoice_id }),
        person.name.as_deref().unwrap_or(&person.sub),
        Some(person.person_id),
    )
    .await?;
    crate::jobs::enqueue(&state, job_id).await;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "job_id": job_id, "status": "queued" })),
    ))
}

pub async fn send_reminder(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id, reminder_id)): Path<(Uuid, Uuid, Uuid)>,
    body: Option<Json<SendRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::InvoiceSend).await?;
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

pub async fn list_dispatches(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::InvoiceRead).await?;
    let rows = regnmed_db::list_dispatches(&state.pool, company_id, invoice_id).await?;
    let mut dispatches = Vec::with_capacity(rows.len());
    for u in &rows {
        let peppol = if u.channel == "peppol" {
            regnmed_db::peppol::dispatch_summary(&state.pool, u.id).await?
        } else {
            None
        };
        dispatches.push(json!({
            "utsendelse_id": u.id,
            "reminder_id": u.reminder_id,
            "kanal": u.channel,
            "to": u.to,
            "subject": u.subject,
            "sent_by": u.sent_by,
            "sent_at": u.sent_at.to_rfc3339(),
            "status": peppol.as_ref().map(|p| p.status.as_str()),
            "detalj": peppol.as_ref().map(|p| p.detail.as_str()),
            "kan_sendes_pa_nytt": peppol.as_ref().is_some_and(|p| p.can_resend),
        }));
    }
    Ok(Json(json!({ "utsendelser": dispatches })))
}

/// Queues the invitation mail (#66) and logs it, returning why it did not
/// go if it did not.
///
/// **This can never fail an invitation.** The invitation is the grant;
/// the mail is only the notification of it. If NATS is down, or the
/// stream refuses the publish, the access must still exist — otherwise a
/// queue outage would take membership administration down with it. So the
/// caller decides what to do with the reason: `invite` reports it in the
/// response, while an explicit resend turns it into a 400.
pub async fn try_send_invitation(
    state: &AppState,
    person: &AuthPerson,
    company_id: Uuid,
    invitation_id: Uuid,
) -> Result<(), String> {
    let Some(js) = &state.mailq else {
        return Err("e-postutsendelse er ikke konfigurert (NATS_URL)".into());
    };
    let payload = regnmed_db::invitation_email_payload(
        &state.pool,
        company_id,
        invitation_id,
        state.portal_base.as_deref(),
    )
    .await
    .map_err(|e| format!("{e:#}"))?;

    let id = Uuid::now_v7();
    publish(js, &OutboundMail::from_payload(id, &payload))
        .await
        .map_err(|e| format!("kunne ikke legge i utsendelseskøen: {e:#}"))?;
    let sent_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::log_dispatch(
        &state.pool,
        id,
        company_id,
        None,
        None,
        Some(invitation_id),
        &payload.to,
        &payload.subject,
        sent_by,
    )
    .await
    .map_err(|e| format!("{e:#}"))?;
    Ok(())
}

/// "Send it again" for an open invitation — the mail is the only thing
/// that gets repeated, the invitation itself is untouched.
pub async fn resend_invitation(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invitation_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::MemberAdmin).await?;
    try_send_invitation(&state, &person, company_id, invitation_id)
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(json!({ "epost_sendt": true })))
}
