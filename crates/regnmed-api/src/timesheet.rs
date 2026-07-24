//! Timeføring (docs/timer.md, #38):
//!
//! - GET/POST /companies/{id}/timesheet                min uke / register
//! - PUT/DELETE /companies/{id}/timesheet/{eid}        own entries (admin: all)
//! - GET  /companies/{id}/timesheet/summary            per prosjekt
//! - GET  /companies/{id}/timesheet/unbilled           fakturagrunnlaget
//! - POST /companies/{id}/timesheet/invoice            bill the hours
//! - GET/PUT /companies/{id}/timesheet/lock            månedslås (PUT admin)
//!
//! Writing requires bokforing or admin; everyone writes their OWN
//! hours (admins may correct anyone's). Locking is admin-only.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};

async fn access_level(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
) -> Result<String, ApiError> {
    regnmed_db::company_access(&state.pool, person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)
}

async fn require_write(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
) -> Result<bool, ApiError> {
    let access = access_level(state, person_id, company_id).await?;
    if access == "les" {
        return Err(ApiError::Forbidden(
            "read-only access — timeføring requires bokforing",
        ));
    }
    Ok(access == "admin")
}

#[derive(Deserialize)]
pub struct EntryRequest {
    dato: chrono::NaiveDate,
    minutter: i32,
    beskrivelse: String,
    prosjekt: Option<String>,
    #[serde(default)]
    fakturerbar: bool,
    timesats_ore: Option<i64>,
}

impl EntryRequest {
    fn draft(self) -> regnmed_db::TimeEntryDraft {
        regnmed_db::TimeEntryDraft {
            dato: self.dato,
            minutter: self.minutter,
            beskrivelse: self.beskrivelse,
            prosjekt: self.prosjekt,
            fakturerbar: self.fakturerbar,
            timesats_ore: self.timesats_ore,
        }
    }
}

pub async fn create(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<EntryRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_write(&state, person.person_id, company_id).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let id = regnmed_db::create_time_entry(
        &state.pool,
        company_id,
        person.person_id,
        &request.draft(),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "entry_id": id })))
}

pub async fn update(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, entry_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<EntryRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let admin = require_write(&state, person.person_id, company_id).await?;
    regnmed_db::update_time_entry(
        &state.pool,
        company_id,
        entry_id,
        person.person_id,
        !admin,
        &request.draft(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "updated": true })))
}

pub async fn delete(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, entry_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let admin = require_write(&state, person.person_id, company_id).await?;
    regnmed_db::delete_time_entry(&state.pool, company_id, entry_id, person.person_id, !admin)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
pub struct RangeQuery {
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(range): Query<RangeQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    access_level(&state, person.person_id, company_id).await?;
    let entries = regnmed_db::list_time_entries(
        &state.pool,
        company_id,
        person.person_id,
        range.from,
        range.to,
    )
    .await?;
    let lock = regnmed_db::timesheet_lock(&state.pool, company_id).await?;
    Ok(Json(json!({
        "locked_through": lock.map(|d| d.to_string()),
        "entries": entries.iter().map(|e| json!({
            "entry_id": e.id,
            "person": e.person_name,
            "own": e.own,
            "dato": e.dato.to_string(),
            "minutter": e.minutter,
            "beskrivelse": e.beskrivelse,
            "prosjekt": e.prosjekt,
            "fakturerbar": e.fakturerbar,
            "timesats_ore": e.timesats_ore,
            "invoice_no": e.invoice_no,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn summary(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(range): Query<RangeQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    access_level(&state, person.person_id, company_id).await?;
    let rows = regnmed_db::timesheet_summary(&state.pool, company_id, range.from, range.to).await?;
    Ok(Json(json!({
        "prosjekter": rows.iter().map(|r| json!({
            "prosjekt": r.prosjekt,
            "minutter": r.minutter,
            "fakturerbare_minutter": r.fakturerbare_minutter,
            "ufakturert_ore": r.ufakturert_ore,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize, Default)]
pub struct UnbilledQuery {
    prosjekt: Option<String>,
    through: Option<chrono::NaiveDate>,
}

pub async fn unbilled(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<UnbilledQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    access_level(&state, person.person_id, company_id).await?;
    let groups = regnmed_db::unbilled_groups(
        &state.pool,
        company_id,
        query.prosjekt.as_deref(),
        query.through,
    )
    .await?;
    Ok(Json(json!({
        "groups": groups.iter().map(|g| json!({
            "prosjekt": g.prosjekt,
            "timesats_ore": g.timesats_ore,
            "minutter": g.minutter,
            "entries": g.entry_ids.len(),
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct BillRequest {
    party_no: String,
    prosjekt: Option<String>,
    through: Option<chrono::NaiveDate>,
    vat_code: Option<String>,
    invoice_date: Option<chrono::NaiveDate>,
    due_date: Option<chrono::NaiveDate>,
}

pub async fn bill(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<BillRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_write(&state, person.person_id, company_id).await?;
    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let invoice_date = request.invoice_date.unwrap_or(today);
    let due_date = request
        .due_date
        .unwrap_or(invoice_date + chrono::Days::new(14));
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let issued = regnmed_db::bill_hours(
        &state.pool,
        company_id,
        &request.party_no,
        request.prosjekt.as_deref(),
        request.through,
        request.vat_code.as_deref(),
        invoice_date,
        due_date,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "invoice_id": issued.invoice_id,
        "invoice_no": issued.invoice_no,
        "kid": issued.kid,
        "gross_ore": issued.gross_ore,
        "voucher": format!("{}-{}", issued.fiscal_year, issued.voucher_number),
    })))
}

pub async fn get_lock(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    access_level(&state, person.person_id, company_id).await?;
    let lock = regnmed_db::timesheet_lock(&state.pool, company_id).await?;
    Ok(Json(
        json!({ "locked_through": lock.map(|d| d.to_string()) }),
    ))
}

#[derive(Deserialize)]
pub struct LockRequest {
    locked_through: chrono::NaiveDate,
    note: Option<String>,
}

pub async fn set_lock(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<LockRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let access = access_level(&state, person.person_id, company_id).await?;
    if access != "admin" {
        return Err(ApiError::Forbidden("timelås requires admin"));
    }
    let locked_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::set_timesheet_lock(
        &state.pool,
        company_id,
        request.locked_through,
        locked_by,
        request.note.as_deref(),
    )
    .await?;
    Ok(Json(
        json!({ "locked_through": request.locked_through.to_string() }),
    ))
}
