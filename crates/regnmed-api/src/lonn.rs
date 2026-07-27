//! Lønn (docs/lonn.md, #46 første del):
//!
//! - GET/POST /companies/{id}/employees          ansattregister
//! - GET/POST /companies/{id}/payroll            lønnskjøringer
//! - GET  /companies/{id}/payroll/{rid}/slip/{eid}  lønnsslipp (PDF)
//! - GET      /companies/{id}/payroll/preview    beregning uten bokføring
//!
//! Lesing krever tilgang; å registrere ansatte og kjøre lønn krever
//! bokforing eller admin. Fødselsnummer forlater aldri dette laget —
//! listen bærer fødselsdato, som er alt noen trenger å se for å
//! kjenne igjen en ansatt.

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
            "read-only access — lønn requires bokforing",
        ));
    }
    Ok(())
}

fn ansatt_json(a: &regnmed_db::lonn::Ansatt) -> serde_json::Value {
    json!({
        "id": a.id,
        "navn": a.navn,
        "stilling": a.stilling,
        // Fødselsdato, ikke fødselsnummer.
        "fodselsdato": a.fodselsdato,
        "ansatt_fra": a.ansatt_fra,
        "ansatt_til": a.ansatt_til,
        "manedslonn_ore": a.manedslonn_ore,
        "timelonn_ore": a.timelonn_ore,
        "trekk_type": a.trekk_type,
        "trekk_prosent_bp": a.trekk_prosent_bp,
        "trekk_tabell": a.trekk_tabell,
        "feriepenger_bp": a.feriepenger_bp,
        "bank_account": a.bank_account,
        "note": a.note,
    })
}

pub async fn list_employees(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let ansatte = regnmed_db::lonn::list_ansatte(&state.pool, company_id).await?;
    Ok(Json(json!({
        "ansatte": ansatte.iter().map(ansatt_json).collect::<Vec<_>>()
    })))
}

#[derive(Deserialize)]
pub struct CreateEmployeeRequest {
    fodselsnummer: String,
    navn: String,
    stilling: Option<String>,
    ansatt_fra: chrono::NaiveDate,
    manedslonn_ore: Option<i64>,
    timelonn_ore: Option<i64>,
    #[serde(default = "prosent")]
    trekk_type: String,
    trekk_prosent_bp: Option<i32>,
    trekk_tabell: Option<i32>,
    /// Ferieloven §10: 1020 as a minimum, 1250 from the year the
    /// employee turns 60, higher on tariff.
    #[serde(default = "lovens_minimum")]
    feriepenger_bp: i32,
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
    require_access(&state, person.person_id, company_id, true).await?;
    let id = regnmed_db::lonn::create_ansatt(
        &state.pool,
        company_id,
        &regnmed_db::lonn::NyAnsatt {
            fodselsnummer: request.fodselsnummer,
            navn: request.navn,
            stilling: request.stilling,
            ansatt_fra: request.ansatt_fra,
            manedslonn_ore: request.manedslonn_ore,
            timelonn_ore: request.timelonn_ore,
            trekk_type: request.trekk_type,
            trekk_prosent_bp: request.trekk_prosent_bp,
            trekk_tabell: request.trekk_tabell,
            feriepenger_bp: request.feriepenger_bp,
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

fn kjoring_json(k: &regnmed_db::lonn::Lonnskjoring) -> serde_json::Value {
    json!({
        "id": k.id,
        "ar": k.ar,
        "maned": k.maned,
        "utbetalt_dato": k.utbetalt_dato,
        "sone": k.sone,
        "brutto_ore": k.sum.brutto_ore,
        "feriepenger_utbetalt_ore": k.sum.feriepenger_utbetalt_ore,
        "forskuddstrekk_ore": k.sum.forskuddstrekk_ore,
        "netto_ore": k.sum.netto_ore,
        "feriepengeavsetning_ore": k.sum.feriepengeavsetning_ore,
        "aga_ore": k.sum.aga_ore,
        "lonnskostnad_ore": k.sum.lonnskostnad_ore(),
        "voucher_id": k.voucher_id,
        "ansatte": k.ansatte.iter().map(|(id, navn)| json!({
            "employee_id": id, "navn": navn,
        })).collect::<Vec<_>>(),
    })
}

pub async fn list_payroll(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<YearQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let runs = regnmed_db::lonn::list_kjoringer(&state.pool, company_id, q.year).await?;
    Ok(Json(json!({
        "kjoringer": runs.iter().map(kjoring_json).collect::<Vec<_>>()
    })))
}

#[derive(Deserialize)]
pub struct PayrollLine {
    employee_id: Uuid,
    /// Omitted uses the employee's månedslønn.
    brutto_ore: Option<i64>,
    #[serde(default)]
    feriepenger_ore: i64,
}

#[derive(Deserialize)]
pub struct RunPayrollRequest {
    ar: i32,
    maned: u32,
    utbetalt_dato: chrono::NaiveDate,
    /// Arbeidsgiveravgiftssone: I, II, III, IV, IVa or V. Ia is refused
    /// — its reduced rate is bounded by a yearly fribeløp regnmed
    /// cannot see in full (docs/lonn.md).
    sone: String,
    linjer: Vec<PayrollLine>,
}

pub async fn run_payroll(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<RunPayrollRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let poster: Vec<_> = request
        .linjer
        .iter()
        .map(|l| regnmed_db::lonn::Lonnspost {
            employee_id: l.employee_id,
            brutto_ore: l.brutto_ore,
            feriepenger_ore: l.feriepenger_ore,
        })
        .collect();
    let kjoring = regnmed_db::lonn::kjor_lonn(
        &state.pool,
        company_id,
        request.ar,
        request.maned,
        request.utbetalt_dato,
        &request.sone,
        &poster,
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    Ok(Json(json!({
        "kjoring": kjoring_json(&kjoring),
        "linjer": kjoring.linjer.iter().map(|(id, navn, b, avsetning)| json!({
            "employee_id": id,
            "navn": navn,
            "brutto_ore": b.brutto_ore,
            "feriepenger_ore": b.feriepenger_ore,
            "forskuddstrekk_ore": b.forskuddstrekk_ore,
            "netto_ore": b.netto_ore,
            "feriepengeavsetning_ore": avsetning,
            "halv_trekk": b.halv_trekk,
        })).collect::<Vec<_>>(),
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
    require_access(&state, person.person_id, company_id, false).await?;
    let input = regnmed_db::lonn::lonnsslipp(&state.pool, company_id, run_id, employee_id)
        .await
        .map_err(|_| ApiError::NotFound)?;
    let filnavn = format!("lonnsslipp-{}-{:02}.pdf", input.ar, input.maned);
    let pdf = regnmed_core::lonnsslipp::render_lonnsslipp(&input);
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{filnavn}\""),
            ),
        ],
        pdf,
    )
        .into_response())
}
