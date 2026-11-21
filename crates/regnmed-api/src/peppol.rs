//! The Peppol webhook and the sender setup (docs/efaktura.md §3.6, §3.7).
//!
//! The API only checks the signature and queues the raw event, or queues
//! a job. Workers process both, and no provider call is ever made here.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use regnmed_db::job::{self, Kind};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

const MAX_BODY: usize = 64 * 1024;

pub async fn recommand_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    // Recommand treats a 4xx as final, so a missing dependency is a 503.
    let (Some(secret), Some(js)) = (&state.peppol_webhook_secret, &state.mailq) else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    if body.len() > MAX_BODY {
        return StatusCode::PAYLOAD_TOO_LARGE;
    }
    let signature = headers
        .get("x-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !regnmed_gov::recommand::verify_webhook(&body, signature, secret) {
        return StatusCode::UNAUTHORIZED;
    }
    let message_id = headers
        .get("x-idempotency-key")
        .and_then(|v| v.to_str().ok())
        .filter(|key| !key.is_empty())
        .map_or_else(|| hex::encode(Sha256::digest(&body)), str::to_string);
    match regnmed_mail::peppol_events::publish(js, &message_id, body.to_vec()).await {
        Ok(()) => StatusCode::OK,
        Err(e) => {
            eprintln!("could not queue a Peppol event: {e:#}");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

/// Local state only, so the portal can show it without asking the provider.
pub async fn sender_status(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::CompanyAdmin).await?;
    let sender = regnmed_db::peppol::load_sender(&state.pool, company_id).await?;
    Ok(Json(json!({
        "tilgjengelig": state.peppol_webhook_secret.is_some(),
        "aktivert": sender.is_some(),
        "er_verifisert": sender.is_some_and(|s| s.is_verified),
    })))
}

/// Registers the company as a send-only participant, or fetches a new
/// verification link. The link is in the job's result.
pub async fn setup_sender(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    require(&state, person.person_id, company_id, Right::CompanyAdmin).await?;
    if state.peppol_webhook_secret.is_none() {
        return Err(ApiError::BadRequest(
            "EHF-utsendelse er ikke slått på ennå.".into(),
        ));
    }
    let job_id = job::create_deduped(
        &state.pool,
        company_id,
        Kind::PeppolSenderSetup,
        &company_id.to_string(),
        &json!({}),
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
