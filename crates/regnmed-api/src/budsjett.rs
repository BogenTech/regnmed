//! Budget and variance report (docs/budsjett.md, #41):
//!
//! - GET    /companies/{id}/budgets[?year=]        versions with status
//! - POST   /companies/{id}/budgets                new draft (optionally from last year ±X %)
//! - GET    /companies/{id}/budgets/{bid}          the budget with its lines
//! - PUT    /companies/{id}/budgets/{bid}/lines    replace the lines (draft)
//! - POST   /companies/{id}/budgets/{bid}/fastsett freezes the version
//! - DELETE /companies/{id}/budgets/{bid}          discard a draft
//! - GET    /companies/{id}/reports/avvik?year=&budget_id=&t_o_m=
//!
//! Amounts are in presentation sign (income positive, cost positive) —
//! a budget is written the way it is read. Reading is open to every
//! access level (a revisor sees the plan like everything else); changing
//! requires posting access, and fastsettelse is a separate, logged
//! action.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

fn budget_json(b: &regnmed_db::BudgetRow) -> serde_json::Value {
    json!({
        "budget_id": b.id,
        "year": b.year,
        "versjon": b.versjon,
        "navn": b.navn,
        "status": b.status,
        "note": b.note,
        "sum_ore": b.sum_ore,
        "created_by": b.created_by,
        "created_at": b.created_at.to_rfc3339(),
        "fastsatt_by": b.fastsatt_by,
        "fastsatt_at": b.fastsatt_at.map(|t| t.to_rfc3339()),
    })
}

#[derive(Deserialize)]
pub struct YearQuery {
    year: Option<i32>,
}

pub async fn list_budgets(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<YearQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BudsjettLes).await?;
    let budgets = regnmed_db::list_budgets(&state.pool, company_id, query.year).await?;
    Ok(Json(json!({
        "budgets": budgets.iter().map(budget_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateBudgetRequest {
    year: i32,
    navn: Option<String>,
    note: Option<String>,
    /// Seed the lines from this year's ACTUALS…
    fra_ar: Option<i32>,
    /// …scaled by basis points (500 = +5 %).
    justering_bp: Option<i64>,
}

pub async fn create_budget(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateBudgetRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BudsjettSkriv).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let navn = request
        .navn
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| format!("Budsjett {}", request.year));
    let budget_id = regnmed_db::create_budget(
        &state.pool,
        company_id,
        request.year,
        &navn,
        request.note.as_deref(),
        request.fra_ar,
        request.justering_bp.unwrap_or(0),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "budget_id": budget_id })))
}

pub async fn get_budget(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, budget_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BudsjettLes).await?;
    let budget = regnmed_db::get_budget(&state.pool, company_id, budget_id)
        .await
        .map_err(|_| ApiError::NotFound)?;
    let lines = regnmed_db::budget_lines(&state.pool, company_id, budget_id).await?;
    Ok(Json(json!({
        "budget": budget_json(&budget),
        "lines": lines.iter().map(|l| json!({
            "account": l.account_number,
            "account_name": l.account_name,
            "maned": l.maned,
            "belop_ore": l.belop_ore,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct LineRequest {
    account: String,
    maned: i32,
    belop_ore: i64,
}

#[derive(Deserialize)]
pub struct SetLinesRequest {
    lines: Vec<LineRequest>,
}

pub async fn set_lines(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, budget_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<SetLinesRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BudsjettSkriv).await?;
    let lines: Vec<regnmed_db::BudgetLineDraft> = request
        .lines
        .into_iter()
        .map(|l| regnmed_db::BudgetLineDraft {
            account_number: l.account.trim().to_string(),
            maned: l.maned,
            belop_ore: l.belop_ore,
        })
        .collect();
    regnmed_db::set_budget_lines(&state.pool, company_id, budget_id, &lines)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "lines": lines.len() })))
}

pub async fn fastsett(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, budget_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BudsjettSkriv).await?;
    let by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::fastsett_budget(&state.pool, company_id, budget_id, by)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "status": "fastsatt" })))
}

pub async fn delete_budget(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, budget_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BudsjettSkriv).await?;
    regnmed_db::delete_budget(&state.pool, company_id, budget_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
pub struct AvvikQuery {
    year: Option<i32>,
    budget_id: Option<Uuid>,
    /// How far "hittil" reaches; defaults to today's month for the
    /// running year, 12 for a finished one.
    t_o_m: Option<u32>,
}

pub async fn avvik(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<AvvikQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BudsjettLes).await?;
    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let this_year = chrono::Datelike::year(&today);
    let year = query.year.unwrap_or(this_year);
    let t_o_m = query.t_o_m.unwrap_or(if year == this_year {
        chrono::Datelike::month(&today)
    } else {
        12
    });
    let result = regnmed_db::avviksrapport(&state.pool, company_id, year, query.budget_id, t_o_m)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let r = &result.rapport;
    Ok(Json(json!({
        "year": year,
        "t_o_m_maned": r.t_o_m_maned,
        // Which plan these numbers are measured against — never "the
        // budget", always a named version.
        "budsjett": result.budsjett.as_ref().map(budget_json),
        "seksjoner": r.seksjoner.iter().map(|s| json!({
            "heading": s.heading,
            "budsjett_hittil_ore": s.budsjett_hittil_ore,
            "faktisk_hittil_ore": s.faktisk_hittil_ore,
            "avvik_hittil_ore": s.avvik_hittil_ore,
            "budsjett_ar_ore": s.budsjett_ar_ore,
            "linjer": s.linjer.iter().map(|l| json!({
                "account": l.number,
                "name": l.name,
                "budsjett_hittil_ore": l.budsjett_hittil_ore,
                "faktisk_hittil_ore": l.faktisk_hittil_ore,
                "avvik_hittil_ore": l.avvik_hittil_ore,
                "budsjett_ar_ore": l.budsjett_ar_ore,
                "budsjett_maaneder": l.budsjett_maaneder,
                "faktisk_maaneder": l.faktisk_maaneder,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "resultat_budsjett_hittil_ore": r.resultat_budsjett_hittil_ore,
        "resultat_faktisk_hittil_ore": r.resultat_faktisk_hittil_ore,
        "resultat_avvik_hittil_ore": r.resultat_avvik_hittil_ore,
        "resultat_budsjett_ar_ore": r.resultat_budsjett_ar_ore,
        "resultat_budsjett_maaneder": r.resultat_budsjett_maaneder,
        "resultat_faktisk_maaneder": r.resultat_faktisk_maaneder,
    })))
}
