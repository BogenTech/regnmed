//! Payment follow-up (web-first, engagement-guarded):
//!
//! - GET  /companies/{id}/invoices/overdue          overdue, with age bucket
//! - GET  /companies/{id}/invoices/{iid}/reminders  dunning history
//! - POST /companies/{id}/invoices/{iid}/reminders  record a step
//!   (`?preview=true` computes and renders without writing anything)
//! - GET  /companies/{id}/invoices/{iid}/reminders/{rid}?format=tekst
//!
//! Sending a reminder is always an explicit human action — the endpoints
//! propose and record, nothing is sent automatically. Reading is open to
//! every access level; recording requires bokforing or admin.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};
use crate::herding::{Vis, file_response};

pub async fn overdue(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ReminderRead).await?;
    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let rows = regnmed_db::overdue_invoices(&state.pool, company_id, today).await?;
    let sum_for = |bucket: &str| {
        rows.iter()
            .filter(|r| r.bucket == bucket)
            .map(|r| r.remaining_ore)
            .sum::<i64>()
    };
    Ok(Json(json!({
        "per_date": today.to_string(),
        "buckets": {
            "1-14": sum_for("1-14"),
            "15-30": sum_for("15-30"),
            "30+": sum_for("30+"),
        },
        "invoices": rows.iter().map(|r| json!({
            "invoice_id": r.invoice_id,
            "invoice_no": r.invoice_no,
            "party_no": r.party_no,
            "party_name": r.party_name,
            "due_date": r.due_date.to_string(),
            "days_overdue": r.days_overdue,
            "bucket": r.bucket,
            "remaining_ore": r.remaining_ore,
            "last_steg": r.last_steg,
            "last_sent": r.last_sent.map(|d| d.to_string()),
        })).collect::<Vec<_>>(),
    })))
}

pub async fn list_reminders(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ReminderRead).await?;
    let rows = regnmed_db::list_reminders(&state.pool, company_id, invoice_id).await?;
    Ok(Json(json!({
        "reminders": rows.iter().map(|r| json!({
            "reminder_id": r.reminder_id,
            "steg": r.step,
            "sent_date": r.sent_date.to_string(),
            "frist_date": r.grace_date.to_string(),
            "remaining_ore": r.remaining_ore,
            "gebyr_ore": r.fee_ore,
            "rente_ore": r.interest_ore,
            "voucher": r.voucher.map(|(fy, no)| format!("{fy}-{no}")),
            "created_by": r.created_by,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateReminderRequest {
    /// paminnelse | purring | inkassovarsel
    #[serde(rename = "steg")]
    step: String,
    /// Defaults to today.
    sent_date: Option<chrono::NaiveDate>,
    #[serde(rename = "frist_date")]
    grace_date: chrono::NaiveDate,
    /// Defaults to 0 (a fee-free step).
    #[serde(rename = "gebyr_ore")]
    fee_ore: Option<i64>,
    /// Claim accrued late-payment interest. Defaults to false.
    #[serde(default, rename = "med_rente")]
    with_interest: bool,
    /// The debtor is næringsdrivende → the fee ceiling is the standardkompensasjon.
    #[serde(default)]
    naeringsdrivende: bool,
    /// Defaults: fee 3950 (annen driftsrelatert inntekt),
    /// interest 8050 (annen renteinntekt).
    #[serde(rename = "gebyr_account")]
    fee_account: Option<String>,
    #[serde(rename = "rente_account")]
    interest_account: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateQuery {
    #[serde(default)]
    preview: bool,
}

fn result_json(r: &regnmed_db::ReminderResult) -> serde_json::Value {
    json!({
        "reminder_id": r.reminder_id,
        "steg": r.step,
        "sent_date": r.sent_date.to_string(),
        "frist_date": r.grace_date.to_string(),
        "remaining_ore": r.remaining_ore,
        "gebyr_ore": r.fee_ore,
        "maks_gebyr_ore": r.max_fee_ore,
        "rente_ore": r.interest_ore,
        "total_ore": r.total_ore,
        "kid": r.kid,
        "document": r.document,
        "voucher": r.voucher.map(|(fy, no)| format!("{fy}-{no}")),
    })
}

pub async fn create_reminder(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<CreateQuery>,
    Json(request): Json<CreateReminderRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ReminderWrite).await?;
    let draft = regnmed_db::ReminderDraft {
        step: request.step,
        sent_date: request.sent_date,
        grace_date: request.grace_date,
        fee_ore: request.fee_ore.unwrap_or(0),
        with_interest: request.with_interest,
        naeringsdrivende: request.naeringsdrivende,
        fee_account: request.fee_account.unwrap_or_else(|| "3950".into()),
        interest_account: request.interest_account.unwrap_or_else(|| "8050".into()),
    };
    let result = if query.preview {
        regnmed_db::preview_reminder(&state.pool, company_id, invoice_id, &draft).await
    } else {
        let created_by = person.name.as_deref().unwrap_or(&person.sub);
        regnmed_db::create_reminder(&state.pool, company_id, invoice_id, &draft, created_by).await
    }
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(result_json(&result)))
}

#[derive(Deserialize)]
pub struct DocumentQuery {
    format: Option<String>,
}

/// The stored document for one skritt — evidence, re-issuable forever.
pub async fn reminder_document(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id, reminder_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(query): Query<DocumentQuery>,
) -> Result<Response, ApiError> {
    require(&state, person.person_id, company_id, Right::ReminderRead).await?;
    let document = regnmed_db::reminder_document(&state.pool, company_id, invoice_id, reminder_id)
        .await
        .map_err(|_| ApiError::NotFound)?;
    // `?format=pdf`: the stored text rendered deterministically to PDF.
    if query.format.as_deref() == Some("pdf") {
        let pdf = regnmed_core::fakturapdf::render_tekst_pdf(&document);
        return Ok(file_response(
            Vis::Inline,
            &format!("purring_{reminder_id}.pdf"),
            "application/pdf",
            pdf,
        ));
    }
    if query.format.as_deref() == Some("tekst") {
        let filename = format!("purring_{reminder_id}.txt");
        return Ok(file_response(
            Vis::Attachment,
            &filename,
            "text/plain",
            document,
        ));
    }
    Ok(Json(json!({ "document": document })).into_response())
}
