//! Betalingsoppfølging (web-first, engagement-guarded):
//!
//! - GET  /companies/{id}/invoices/overdue                    forfalte m/ aldersintervall
//! - GET  /companies/{id}/invoices/{iid}/reminders            purrehistorikk
//! - POST /companies/{id}/invoices/{iid}/reminders            registrer skritt
//!   (`?preview=true` beregner og rendrer uten å skrive noe)
//! - GET  /companies/{id}/invoices/{iid}/reminders/{rid}?format=tekst
//!
//! Å sende en purring er alltid en eksplisitt menneskelig handling —
//! endepunktene foreslår og registrerer, ingenting sendes automatisk.
//! Lesing er åpen for alle tilgangsnivåer; registrering krever
//! bokforing eller admin.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};

async fn require_access(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
    write: bool,
) -> Result<(), ApiError> {
    let access = regnmed_db::company_access(&state.pool, person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if write && access == "les" {
        return Err(ApiError::Forbidden(
            "read-only access — purring requires bokforing",
        ));
    }
    Ok(())
}

pub async fn overdue(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
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
    require_access(&state, person.person_id, company_id, false).await?;
    let rows = regnmed_db::list_reminders(&state.pool, company_id, invoice_id).await?;
    Ok(Json(json!({
        "reminders": rows.iter().map(|r| json!({
            "reminder_id": r.reminder_id,
            "steg": r.steg,
            "sent_date": r.sent_date.to_string(),
            "frist_date": r.frist_date.to_string(),
            "remaining_ore": r.remaining_ore,
            "gebyr_ore": r.gebyr_ore,
            "rente_ore": r.rente_ore,
            "voucher": r.voucher.map(|(fy, no)| format!("{fy}-{no}")),
            "created_by": r.created_by,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateReminderRequest {
    /// paminnelse | purring | inkassovarsel
    steg: String,
    /// Defaults to today.
    sent_date: Option<chrono::NaiveDate>,
    frist_date: chrono::NaiveDate,
    /// Defaults to 0 (gebyrfritt skritt).
    gebyr_ore: Option<i64>,
    /// Krev påløpt forsinkelsesrente. Defaults to false.
    #[serde(default)]
    med_rente: bool,
    /// Skyldner er næringsdrivende → gebyrtaket er standardkompensasjonen.
    #[serde(default)]
    naeringsdrivende: bool,
    /// Defaults: gebyr 3950 (annen driftsrelatert inntekt),
    /// rente 8050 (annen renteinntekt).
    gebyr_account: Option<String>,
    rente_account: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateQuery {
    #[serde(default)]
    preview: bool,
}

fn result_json(r: &regnmed_db::ReminderResult) -> serde_json::Value {
    json!({
        "reminder_id": r.reminder_id,
        "steg": r.steg,
        "sent_date": r.sent_date.to_string(),
        "frist_date": r.frist_date.to_string(),
        "remaining_ore": r.remaining_ore,
        "gebyr_ore": r.gebyr_ore,
        "maks_gebyr_ore": r.maks_gebyr_ore,
        "rente_ore": r.rente_ore,
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
    require_access(&state, person.person_id, company_id, true).await?;
    let draft = regnmed_db::ReminderDraft {
        steg: request.steg,
        sent_date: request.sent_date,
        frist_date: request.frist_date,
        gebyr_ore: request.gebyr_ore.unwrap_or(0),
        med_rente: request.med_rente,
        naeringsdrivende: request.naeringsdrivende,
        gebyr_account: request.gebyr_account.unwrap_or_else(|| "3950".into()),
        rente_account: request.rente_account.unwrap_or_else(|| "8050".into()),
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
    require_access(&state, person.person_id, company_id, false).await?;
    let document = regnmed_db::reminder_document(&state.pool, company_id, invoice_id, reminder_id)
        .await
        .map_err(|_| ApiError::NotFound)?;
    // `?format=pdf`: the stored text rendered deterministically to PDF.
    if query.format.as_deref() == Some("pdf") {
        let pdf = regnmed_core::fakturapdf::render_tekst_pdf(&document);
        return Ok((
            [
                (header::CONTENT_TYPE, "application/pdf".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("inline; filename=\"purring_{reminder_id}.pdf\""),
                ),
            ],
            pdf,
        )
            .into_response());
    }
    if query.format.as_deref() == Some("tekst") {
        let filename = format!("purring_{reminder_id}.txt");
        return Ok((
            [
                (
                    header::CONTENT_TYPE,
                    "text/plain; charset=utf-8".to_string(),
                ),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{filename}\""),
                ),
            ],
            document,
        )
            .into_response());
    }
    Ok(Json(json!({ "document": document })).into_response())
}
