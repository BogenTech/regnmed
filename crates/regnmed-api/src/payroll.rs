//! Lønn (docs/lonn.md, first part of #46):
//!
//! - GET/POST /companies/{id}/employees             employee register
//! - GET/POST /companies/{id}/payroll               payroll runs
//! - GET  /companies/{id}/payroll/{rid}/slip/{eid}  payslip (PDF)
//! - GET      /companies/{id}/payroll/preview       computation without posting
//!
//! Reading requires access; registering employees and running payroll
//! requires bokforing or admin. The fødselsnummer never leaves this
//! layer: the list carries the birth date, which is all anyone needs in
//! order to recognise an employee.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};
use crate::herding::{Vis, file_response};

fn employee_json(a: &regnmed_db::payroll::Employee) -> serde_json::Value {
    json!({
        "id": a.id,
        "navn": a.name,
        "stilling": a.stilling,
        // Birth date, not fødselsnummer.
        "fodselsdato": a.birth_date,
        "ansatt_fra": a.employed_from,
        "ansatt_til": a.employed_to,
        "manedslonn_ore": a.manedslonn_ore,
        "timelonn_ore": a.timelonn_ore,
        "trekk_type": a.withholding_type,
        "trekk_prosent_bp": a.withholding_percent_bp,
        "trekk_tabell": a.withholding_table,
        "feriepenger_bp": a.holiday_pay_bp,
        "bank_account": a.bank_account,
        "note": a.note,
        // The linked portal user (docs/lonn.md, 0050) — null when the
        // employee has no portal access.
        "person": a.person_id.map(|pid| json!({
            "person_id": pid,
            "navn": a.person_name,
            "epost": a.person_email,
        })),
    })
}

pub async fn list_employees(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollRead).await?;
    let employees = regnmed_db::payroll::list_ansatte(&state.pool, company_id).await?;
    Ok(Json(json!({
        "ansatte": employees.iter().map(employee_json).collect::<Vec<_>>()
    })))
}

#[derive(Deserialize)]
pub struct CreateEmployeeRequest {
    #[serde(rename = "fodselsnummer")]
    nin: String,
    #[serde(rename = "navn")]
    name: String,
    stilling: Option<String>,
    #[serde(rename = "ansatt_fra")]
    employed_from: chrono::NaiveDate,
    manedslonn_ore: Option<i64>,
    timelonn_ore: Option<i64>,
    #[serde(default = "prosent")]
    #[serde(rename = "trekk_type")]
    withholding_type: String,
    #[serde(rename = "trekk_prosent_bp")]
    withholding_percent_bp: Option<i32>,
    #[serde(rename = "trekk_tabell")]
    withholding_table: Option<i32>,
    /// Ferieloven §10: 1020 as a minimum, 1250 from the year the
    /// employee turns 60, higher on tariff.
    #[serde(default = "lovens_minimum")]
    #[serde(rename = "feriepenger_bp")]
    holiday_pay_bp: i32,
    bank_account: Option<String>,
    note: Option<String>,
}

fn prosent() -> String {
    "prosent".into()
}
fn lovens_minimum() -> i32 {
    1020
}

pub async fn create_employee(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateEmployeeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollWrite).await?;
    let id = regnmed_db::payroll::create_employee(
        &state.pool,
        company_id,
        &regnmed_db::payroll::NewEmployee {
            nin: request.nin,
            name: request.name,
            stilling: request.stilling,
            employed_from: request.employed_from,
            manedslonn_ore: request.manedslonn_ore,
            timelonn_ore: request.timelonn_ore,
            withholding_type: request.withholding_type,
            withholding_percent_bp: request.withholding_percent_bp,
            withholding_table: request.withholding_table,
            holiday_pay_bp: request.holiday_pay_bp,
            bank_account: request.bank_account,
            note: request.note,
        },
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "employee_id": id })))
}

#[derive(Deserialize)]
pub struct YearQuery {
    year: Option<i32>,
}

fn kjoring_json(k: &regnmed_db::payroll::Lonnskjoring) -> serde_json::Value {
    json!({
        "id": k.id,
        "ar": k.year,
        "maned": k.month,
        "utbetalt_dato": k.paid_date,
        "sone": k.zone,
        "brutto_ore": k.sum.gross_ore,
        "feriepenger_utbetalt_ore": k.sum.holiday_pay_paid_ore,
        "forskuddstrekk_ore": k.sum.withholding_ore,
        "netto_ore": k.sum.net_ore,
        "feriepengeavsetning_ore": k.sum.holiday_pay_accrual_ore,
        "aga_ore": k.sum.employer_contribution_ore,
        "aga_feriepenger_ore": k.sum.employer_contribution_holiday_pay_ore,
        "lonnskostnad_ore": k.sum.payroll_cost_ore(),
        "voucher_id": k.voucher_id,
        "ansatte": k.employees.iter().map(|(id, name)| json!({
            "employee_id": id, "navn": name,
        })).collect::<Vec<_>>(),
    })
}

pub async fn list_payroll(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<YearQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollRead).await?;
    let runs = regnmed_db::payroll::list_kjoringer(&state.pool, company_id, q.year).await?;
    Ok(Json(json!({
        "kjoringer": runs.iter().map(kjoring_json).collect::<Vec<_>>()
    })))
}

#[derive(Deserialize)]
pub struct PayrollLine {
    employee_id: Uuid,
    /// Omitted uses the employee's månedslønn.
    #[serde(rename = "brutto_ore")]
    gross_ore: Option<i64>,
    #[serde(default)]
    #[serde(rename = "feriepenger_ore")]
    holiday_pay_ore: i64,
    /// Pay from hours logged this month instead. The timesheet month
    /// must be locked first (docs/lonn.md).
    #[serde(default)]
    #[serde(rename = "fra_timer")]
    from_timesheet: bool,
}

#[derive(Deserialize)]
pub struct RunPayrollRequest {
    #[serde(rename = "ar")]
    year: i32,
    #[serde(rename = "maned")]
    month: u32,
    #[serde(rename = "utbetalt_dato")]
    paid_date: chrono::NaiveDate,
    /// Arbeidsgiveravgiftssone: I, II, III, IV, IVa or V. Ia is refused
    /// — its reduced rate is bounded by a yearly fribeløp regnmed
    /// cannot see in full (docs/lonn.md).
    #[serde(rename = "sone")]
    zone: String,
    #[serde(rename = "linjer")]
    lines: Vec<PayrollLine>,
}

pub async fn run_payroll(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<RunPayrollRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollRun).await?;
    let poster: Vec<_> = request
        .lines
        .iter()
        .map(|l| regnmed_db::payroll::PayrollEntry {
            employee_id: l.employee_id,
            gross_ore: l.gross_ore,
            holiday_pay_ore: l.holiday_pay_ore,
            from_timesheet: l.from_timesheet,
        })
        .collect();
    let run = regnmed_db::payroll::run_payroll(
        &state.pool,
        company_id,
        request.year,
        request.month,
        request.paid_date,
        &request.zone,
        &poster,
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    Ok(Json(json!({
        "kjoring": kjoring_json(&run),
        "linjer": run.lines.iter().map(|l| json!({
            "employee_id": l.employee_id,
            "navn": l.name,
            "brutto_ore": l.calculation.gross_ore,
            "feriepenger_ore": l.calculation.holiday_pay_ore,
            "forskuddstrekk_ore": l.calculation.withholding_ore,
            "netto_ore": l.calculation.net_ore,
            "feriepengeavsetning_ore": l.holiday_pay_accrual_ore,
            "aga_feriepenger_ore": l.employer_contribution_holiday_pay_ore,
            "halv_trekk": l.calculation.half_withholding,
        })).collect::<Vec<_>>(),
        "advarsler": run.warnings,
    })))
}

/// The payslip for one employee in one run, as PDF.
///
/// Rendered on demand rather than stored: the payroll line is
/// insert-only, so the same line yields the same bytes forever — and
/// not storing it means one fewer copy of personal data to guard. That
/// is the opposite choice from the faktura PDF, where the document *is*
/// the salgsdokument and must be kept exactly as issued.
pub async fn payslip_pdf(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, run_id, employee_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    // Sin egen slipp får man alltid; andres krever LONNSSLIPP_LES_ALLE.
    // Note this is NOT #55: that issue takes LONNSSLIPP_LES_ALLE OUT of
    // the reading bundle. Here only the scope is added, without changing
    // who has what.
    let role = require(&state, person.person_id, company_id, Right::PayslipReadOwn).await?;
    if !role.has(Right::PayslipReadAll) {
        let owner =
            regnmed_db::payroll::employee_person(&state.pool, company_id, employee_id).await?;
        if owner != Some(person.person_id) {
            return Err(ApiError::NotFound);
        }
    }
    let input = regnmed_db::payroll::payslip(&state.pool, company_id, run_id, employee_id)
        .await
        .map_err(|_| ApiError::NotFound)?;
    let filename = format!("lonnsslipp-{}-{:02}.pdf", input.year, input.month);
    let pdf = regnmed_core::payslip::render_payslip(&input);
    Ok(file_response(
        Vis::Inline,
        &filename,
        "application/pdf",
        pdf,
    ))
}

#[derive(Deserialize)]
pub struct HoursQuery {
    #[serde(rename = "ar")]
    year: i32,
    #[serde(rename = "maned")]
    month: u32,
}

/// What an employee's logged hours come to for a month, and whether the
/// timesheet is locked — so the portal can offer hours-based pay only
/// when it is actually safe to run.
pub async fn payroll_hours(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, employee_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<HoursQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollRead).await?;
    let g =
        regnmed_db::payroll::timegrunnlag(&state.pool, company_id, employee_id, q.year, q.month)
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "minutter": g.minutes,
        "timesats_ore": g.hourly_rate_ore,
        "belop_ore": g.amount_ore,
        "laast": g.laast,
    })))
}

#[derive(Deserialize)]
pub struct LinkRequest {
    person_id: Uuid,
}

/// Manually links an employee to an EXISTING portal user — the fallback
/// when the invitation path (invite with `employee_id`) does not fit.
/// The picker in the portal is limited to the company's members; the
/// guards here hold regardless of what the UI sends: unlinked employee
/// only, humans only, one employee per person (docs/lonn.md).
pub async fn link_employee(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, employee_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<LinkRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollWrite).await?;
    regnmed_db::payroll::link_employee_person(
        &state.pool,
        company_id,
        employee_id,
        request.person_id,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "koblet": true })))
}

/// Clears the link. Re-linking afterwards is a fresh, audited decision.
pub async fn unlink_employee(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, employee_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollWrite).await?;
    regnmed_db::payroll::unlink_employee_person(
        &state.pool,
        company_id,
        employee_id,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "frakoblet": true })))
}

/// Who linked whom, when — read when a link is questioned.
pub async fn employee_link_history(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, employee_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollRead).await?;
    let rader =
        regnmed_db::payroll::employee_link_history(&state.pool, company_id, employee_id).await?;
    Ok(Json(json!({
        "historikk": rader.iter().map(|h| json!({
            "endring": h.change,
            "kilde": h.source,
            "person": h.person_name,
            "utfort_av": h.performed_by,
            "tidspunkt": h.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}
