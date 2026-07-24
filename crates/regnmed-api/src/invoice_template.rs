//! Repeterende faktura (docs/faktura.md, #30):
//!
//! - GET/POST /companies/{id}/invoice-templates          list / create
//! - PUT      /companies/{id}/invoice-templates/{tid}    edit (incl. lines, aktiv)
//! - POST     /companies/{id}/invoice-templates/{tid}/generate   generate now
//! - GET      /companies/{id}/invoice-templates/{tid}/runs       the log
//!
//! Reading is open to every access level; writing and generating
//! require bokforing or admin. The daily CronJob drives scheduled
//! generation; the generate-now endpoint is the same code path.

use axum::Json;
use axum::extract::{Path, State};
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
            "read-only access — templates require bokforing",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct TemplateLineRequest {
    description: String,
    account: Option<String>,
    quantity_milli: Option<i64>,
    unit_price_ore: i64,
    vat_code: Option<String>,
    avdeling: Option<String>,
    prosjekt: Option<String>,
}

fn line_drafts(lines: Vec<TemplateLineRequest>) -> Vec<regnmed_db::TemplateLineDraft> {
    lines
        .into_iter()
        .map(|line| regnmed_db::TemplateLineDraft {
            description: line.description,
            account_number: line.account.unwrap_or_else(|| "3000".into()),
            quantity_milli: line.quantity_milli.unwrap_or(1000),
            unit_price_ore: line.unit_price_ore,
            vat_code: line.vat_code,
            avdeling: line.avdeling,
            prosjekt: line.prosjekt,
        })
        .collect()
}

#[derive(Deserialize)]
pub struct CreateTemplateRequest {
    /// "Gjenta denne": copy customer + lines from an existing invoice
    /// instead of passing party_no/lines.
    from_invoice_id: Option<Uuid>,
    party_no: Option<String>,
    /// manedlig | kvartalsvis | arlig
    intervall: String,
    neste_dato: chrono::NaiveDate,
    slutt_dato: Option<chrono::NaiveDate>,
    /// Days from fakturadato to forfall; default 14.
    forfall_dager: Option<i32>,
    /// Mark generated invoices for sending (a human still sends).
    #[serde(default)]
    merk_utsendelse: bool,
    #[serde(default)]
    lines: Vec<TemplateLineRequest>,
}

pub async fn create(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateTemplateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let result = match request.from_invoice_id {
        Some(invoice_id) => {
            regnmed_db::create_template_from_invoice(
                &state.pool,
                company_id,
                invoice_id,
                &request.intervall,
                request.neste_dato,
                request.forfall_dager.unwrap_or(14),
                request.merk_utsendelse,
                created_by,
            )
            .await
        }
        None => {
            let party_no = request.party_no.ok_or_else(|| {
                ApiError::BadRequest("party_no or from_invoice_id required".into())
            })?;
            regnmed_db::create_template(
                &state.pool,
                company_id,
                &regnmed_db::TemplateDraft {
                    party_no,
                    intervall: request.intervall,
                    neste_dato: request.neste_dato,
                    slutt_dato: request.slutt_dato,
                    forfall_dager: request.forfall_dager.unwrap_or(14),
                    merk_utsendelse: request.merk_utsendelse,
                    lines: line_drafts(request.lines),
                },
                created_by,
            )
            .await
        }
    };
    let template_id = result.map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "template_id": template_id })))
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let templates = regnmed_db::list_templates(&state.pool, company_id).await?;
    Ok(Json(json!({
        "templates": templates.iter().map(|t| json!({
            "template_id": t.id,
            "party_no": t.party_no,
            "party_name": t.party_name,
            "intervall": t.intervall,
            "neste_dato": t.neste_dato.to_string(),
            "slutt_dato": t.slutt_dato.map(|d| d.to_string()),
            "forfall_dager": t.forfall_dager,
            "merk_utsendelse": t.merk_utsendelse,
            "active": t.active,
            "sum_netto_ore": t.sum_netto_ore,
            "runs": t.runs,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct UpdateTemplateRequest {
    intervall: Option<String>,
    neste_dato: Option<chrono::NaiveDate>,
    /// Present-but-null clears the sluttdato.
    #[serde(default, with = "double_option")]
    slutt_dato: Option<Option<chrono::NaiveDate>>,
    forfall_dager: Option<i32>,
    merk_utsendelse: Option<bool>,
    active: Option<bool>,
    lines: Option<Vec<TemplateLineRequest>>,
}

/// serde helper: distinguishes an absent field from an explicit null.
mod double_option {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        Option::<T>::deserialize(de).map(Some)
    }
}

pub async fn update(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, template_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateTemplateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let lines = request.lines.map(line_drafts);
    regnmed_db::update_template(
        &state.pool,
        company_id,
        template_id,
        request.intervall.as_deref(),
        request.neste_dato,
        request.slutt_dato,
        request.forfall_dager,
        request.merk_utsendelse,
        request.active,
        lines.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "updated": true })))
}

/// Generate-now: the same code path as the daily CronJob, for one
/// template. Generates every due period (catch-up included).
pub async fn generate(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, template_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let mut generated = Vec::new();
    loop {
        match regnmed_db::generate_one(&state.pool, company_id, template_id, today)
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?
        {
            Some(outcome) => {
                let failed = outcome.invoice_no.is_none();
                let detail = outcome.detail.clone();
                generated.push(json!({
                    "generated_for": outcome.generated_for.to_string(),
                    "invoice_no": outcome.invoice_no,
                    "detail": outcome.detail,
                }));
                if failed {
                    return Err(ApiError::BadRequest(
                        detail.unwrap_or_else(|| "generering feilet".into()),
                    ));
                }
            }
            None => break,
        }
    }
    Ok(Json(json!({ "generated": generated })))
}

pub async fn runs(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, template_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let rows = regnmed_db::list_runs(&state.pool, company_id, template_id).await?;
    Ok(Json(json!({
        "runs": rows.iter().map(|r| json!({
            "generated_for": r.generated_for.to_string(),
            "invoice_no": r.invoice_no,
            "til_utsendelse": r.til_utsendelse,
            "detail": r.detail,
            "at": r.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}
