//! Lønn (docs/lonn.md, first part of #46):
//!
//! - GET/POST /companies/{id}/employees             employee register
//! - PUT  /companies/{id}/employees/{eid}/employment  arbeidsforhold for the a-melding
//! - GET/POST /companies/{id}/payroll               payroll runs
//! - GET  /companies/{id}/payroll/{rid}/slip/{eid}  payslip (PDF)
//! - GET  /companies/{id}/payroll/{rid}/amelding     a-melding preview / XML
//! - GET/POST /companies/{id}/payroll/{rid}/amelding/submissions   send + record
//! - POST /companies/{id}/payroll/{rid}/amelding/submissions/{sid}/feedback
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
        "manedslonn_ore": a.monthly_salary_ore,
        "timelonn_ore": a.hourly_wage_ore,
        "trekk_type": a.withholding_type,
        "trekk_prosent_bp": a.withholding_percent_bp,
        "trekk_tabell": a.withholding_table,
        "trekk_12_maneder": a.withholding_twelve_months,
        "frikort_grense_ore": a.tax_free_card_limit_ore,
        "feriepenger_bp": a.holiday_pay_bp,
        "bank_account": a.bank_account,
        "note": a.note,
        // The arbeidsforhold facts the a-melding requires (0073), and
        // which of them are still missing — named as the a-melding's own
        // refusal names them.
        "arbeidsforhold": details_json(&a.details),
        "amelding_mangler": a.amelding_missing(),
        // The linked portal user (docs/lonn.md, 0050) — null when the
        // employee has no portal access.
        "person": a.person_id.map(|pid| json!({
            "person_id": pid,
            "navn": a.person_name,
            "epost": a.person_email,
        })),
    })
}

fn details_json(d: &regnmed_db::payroll::EmploymentDetails) -> serde_json::Value {
    json!({
        "virksomhet_orgnr": d.business_unit_orgnumber,
        "yrkeskode": d.occupation_code,
        "stillingsprosent_bp": d.position_percent_bp,
        "timer_per_uke_hundredeler": d.full_time_hours_per_week_hundredths,
        "arbeidstidsordning": d.working_hours_scheme,
        "ansettelsesform": d.employment_form,
        "siste_lonnsendring": d.last_salary_change,
    })
}

/// The arbeidsforhold fields as they arrive on the wire — on employee
/// registration and on its own PUT.
#[derive(Deserialize, Default)]
pub struct EmploymentDetailsRequest {
    #[serde(rename = "virksomhet_orgnr")]
    business_unit_orgnumber: Option<String>,
    #[serde(rename = "yrkeskode")]
    occupation_code: Option<String>,
    #[serde(rename = "stillingsprosent_bp")]
    position_percent_bp: Option<i32>,
    #[serde(rename = "timer_per_uke_hundredeler")]
    full_time_hours_per_week_hundredths: Option<i32>,
    #[serde(rename = "arbeidstidsordning")]
    working_hours_scheme: Option<String>,
    #[serde(rename = "ansettelsesform")]
    employment_form: Option<String>,
    #[serde(rename = "siste_lonnsendring")]
    last_salary_change: Option<chrono::NaiveDate>,
}

impl EmploymentDetailsRequest {
    fn into_details(self) -> regnmed_db::payroll::EmploymentDetails {
        // An empty string from a form field means "not given", not a code.
        let text = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        regnmed_db::payroll::EmploymentDetails {
            business_unit_orgnumber: text(self.business_unit_orgnumber),
            occupation_code: text(self.occupation_code),
            position_percent_bp: self.position_percent_bp,
            full_time_hours_per_week_hundredths: self.full_time_hours_per_week_hundredths,
            working_hours_scheme: text(self.working_hours_scheme),
            employment_form: text(self.employment_form),
            last_salary_change: self.last_salary_change,
        }
    }
}

pub async fn list_employees(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollRead).await?;
    let employees = regnmed_db::payroll::list_ansatte(&state.pool, company_id).await?;
    Ok(Json(json!({
        "ansatte": employees.iter().map(employee_json).collect::<Vec<_>>(),
        // The kodeliste values the a-melding accepts, served from the one
        // copy the renderer checks against.
        "koder": {
            "arbeidstidsordning": regnmed_core::amelding::WORKING_HOURS_SCHEMES,
            "ansettelsesform": regnmed_core::amelding::EMPLOYMENT_FORMS,
        },
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
    #[serde(rename = "manedslonn_ore")]
    monthly_salary_ore: Option<i64>,
    #[serde(rename = "timelonn_ore")]
    hourly_wage_ore: Option<i64>,
    #[serde(default = "percent")]
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
    #[serde(flatten)]
    details: EmploymentDetailsRequest,
}

fn percent() -> String {
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
            monthly_salary_ore: request.monthly_salary_ore,
            hourly_wage_ore: request.hourly_wage_ore,
            withholding_type: request.withholding_type,
            withholding_percent_bp: request.withholding_percent_bp,
            withholding_table: request.withholding_table,
            holiday_pay_bp: request.holiday_pay_bp,
            bank_account: request.bank_account,
            note: request.note,
            details: request.details.into_details(),
        },
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "employee_id": id })))
}

/// Sets the arbeidsforhold facts the a-melding requires. A full
/// replacement: a field left out is cleared, so the form can undo a
/// mistake.
pub async fn update_employment(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, employee_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<EmploymentDetailsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollWrite).await?;
    regnmed_db::payroll::update_employment_details(
        &state.pool,
        company_id,
        employee_id,
        &request.into_details(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "updated": true })))
}

#[derive(Deserialize)]
pub struct AmeldingQuery {
    /// `json` (default) previews; `xml` downloads the melding itself.
    format: Option<String>,
}

/// The a-melding for one stored payroll run (docs/lonn.md).
///
/// The JSON form lists EVERYTHING that blocks delivery, so the
/// payroll person can fix it in one pass; `format=xml` returns the
/// melding only when that list is empty. Submission is not here: it
/// needs the Altinn systembruker (#180, docs/gov.md).
///
/// `LONN_KJOR`, not `LONN_LES`: the file carries every employee's
/// fødselsnummer, which the rest of the payroll API never shows — the
/// person who runs payroll and files the melding needs it, a reader
/// (or a revisor) does not.
pub async fn amelding(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, run_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<AmeldingQuery>,
) -> Result<Response, ApiError> {
    use axum::response::IntoResponse;
    require(&state, person.person_id, company_id, Right::PayrollRun).await?;
    let draft =
        regnmed_db::payroll::amelding_for_run(&state.pool, company_id, run_id, chrono::Utc::now())
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let mut hindringer = draft.hindrances;
    // The renderer's own verdict only once ours is clean — otherwise it
    // repeats the first item of the list in other words.
    let xml = if hindringer.is_empty() {
        regnmed_core::amelding::render(&draft.melding)
            .map_err(|e| hindringer.push(e.to_string()))
            .ok()
    } else {
        None
    };
    let m = &draft.melding;
    if q.format.as_deref() == Some("xml") {
        let Some(xml) = xml else {
            return Err(ApiError::BadRequest(hindringer.join("\n")));
        };
        let filename = format!(
            "a-melding-{}-{:02}.xml",
            m.payment_date.format("%Y"),
            m.payment_date.format("%m")
        );
        return Ok(file_response(
            Vis::Attachment,
            &filename,
            "application/xml",
            xml,
        ));
    }
    Ok(Json(json!({
        "leverbar": hindringer.is_empty(),
        "hindringer": hindringer,
        // The month of the PAYMENT date, which is what the melding reports.
        "kalendermaaned": m.payment_date.format("%Y-%m").to_string(),
        "inntektsmottakere": m.recipients.len(),
        "arbeidsgiveravgift_ore": m.employer_contribution_ore,
    }))
    .into_response())
}

/// Skatteetatens verdict on THIS melding inside a tilbakemelding (which
/// covers the whole month): its `mottakstatus` and `avvik`.
fn verdict(feedback: &serde_json::Value, melding_id: &str) -> serde_json::Value {
    let received = feedback["mottak"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|m| m["mottattLeveranse"].as_array().into_iter().flatten())
        .find(|l| l["meldingsId"] == melding_id);
    match received {
        Some(l) => json!({ "mottakstatus": l["mottakstatus"], "avvik": l["avvik"] }),
        None => json!({ "mottakstatus": null, "avvik": null }),
    }
}

fn submission_json(
    s: &regnmed_db::amelding_submission::Submission,
    status: &regnmed_db::amelding_submission::SubmissionStatus,
) -> serde_json::Value {
    use regnmed_db::amelding_submission::SubmissionStatus as S;
    let mut v = json!({
        "id": s.id,
        "melding_id": s.melding_id,
        "periode": s.period,
        "erstatter_melding_id": s.replaces_melding_id,
        "opprettet": s.created_at,
        "opprettet_av": s.created_by,
        "status": status.slug(),
    });
    match status {
        S::NotDelivered { last_error } => v["siste_feil"] = json!(last_error),
        S::Delivered {
            dialog_id,
            forsendelse_id,
        } => {
            v["dialog_id"] = json!(dialog_id);
            v["forsendelse_id"] = json!(forsendelse_id);
        }
        S::Feedback {
            dialog_id,
            forsendelse_id,
            feedback,
        } => {
            v["dialog_id"] = json!(dialog_id);
            v["forsendelse_id"] = json!(forsendelse_id);
            v["tilbakemelding"] = verdict(feedback, &s.melding_id);
        }
    }
    v
}

pub async fn list_amelding_submissions(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, run_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollRead).await?;
    let list =
        regnmed_db::amelding_submission::list_submissions(&state.pool, company_id, run_id).await?;
    Ok(Json(json!({
        "innsendinger": list.iter().map(|(s, st)| submission_json(s, st)).collect::<Vec<_>>(),
        "innsending_mulig": state.amelding.is_some(),
    })))
}

#[derive(Deserialize, Default)]
pub struct SubmitRequest {
    /// Send a run that is already delivered as a replacement melding.
    #[serde(default, rename = "erstatt")]
    replace: bool,
}

/// Sends the run's a-melding to Skatteetaten (docs/lonn.md,
/// «Innsendingssporet»). The submission is STORED before it is sent: an
/// undelivered one is resent as it is (same body, same idempotency key),
/// and a delivered run is only sent again as an explicit replacement —
/// two meldinger for one month add up at Skatteetaten.
pub async fn submit_amelding(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, run_id)): Path<(Uuid, Uuid)>,
    request: Option<Json<SubmitRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollRun).await?;
    let Some(rail) = state.amelding.clone() else {
        return Err(ApiError::BadRequest(
            "innsending av a-melding er ikke satt opp på denne serveren (Maskinporten)".into(),
        ));
    };
    crate::terms::require_accepted(&state, company_id).await?;
    let replace = request.map(|Json(r)| r.replace).unwrap_or_default();
    let by = person.display();
    let s = regnmed_db::amelding_submission::prepare_submission(
        &state.pool,
        company_id,
        run_id,
        replace,
        by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let orgnumber = regnmed_db::company_settings(&state.pool, company_id)
        .await?
        .orgnumber;
    match rail
        .client
        .submit(
            &rail.tokens,
            &orgnumber,
            &s.period,
            s.body.clone(),
            s.idempotency_key,
        )
        .await
    {
        Ok(receipt) => {
            regnmed_db::amelding_submission::record_receipt(
                &state.pool,
                s.id,
                &receipt.dialog_id,
                &receipt.forsendelse_id,
                by,
            )
            .await?;
            ensure_event_subscription(&state, &rail, company_id, &orgnumber, by).await;
        }
        Err(e) => {
            regnmed_db::amelding_submission::record_failure(&state.pool, s.id, &e.to_string(), by)
                .await?;
            return Err(ApiError::BadRequest(format!(
                "Skatteetaten tok ikke imot a-meldingen: {e}. Den er lagret og sendes uendret \
                 (samme idempotensnøkkel) neste gang du sender — den telles ikke dobbelt."
            )));
        }
    }
    let (s, status) =
        regnmed_db::amelding_submission::submission(&state.pool, company_id, s.id).await?;
    Ok(Json(submission_json(&s, &status)))
}

/// Fetches Skatteetatens tilbakemelding for a delivered submission and
/// stores it. Answers with the submission as it stands — still
/// `levert` when Skatteetaten has not answered yet.
/// Fetches the feedback for a delivered submission and stores it when
/// Skatteetaten has produced it. Shared by the button and the webhook.
async fn fetch_feedback(
    state: &AppState,
    rail: &crate::AmeldingRail,
    company_id: Uuid,
    s: &regnmed_db::amelding_submission::Submission,
    status: &regnmed_db::amelding_submission::SubmissionStatus,
    by: &str,
) -> anyhow::Result<()> {
    let regnmed_db::amelding_submission::SubmissionStatus::Delivered {
        dialog_id,
        forsendelse_id,
    } = status
    else {
        return Ok(());
    };
    let orgnumber = regnmed_db::company_settings(&state.pool, company_id)
        .await?
        .orgnumber;
    let receipt = regnmed_gov::amelding::Receipt {
        dialog_id: dialog_id.clone(),
        forsendelse_id: forsendelse_id.clone(),
        meldings_id: s.melding_id.clone(),
    };
    if let Some(feedback) = rail
        .client
        .feedback(&rail.tokens, &orgnumber, &receipt)
        .await?
    {
        regnmed_db::amelding_submission::record_feedback(&state.pool, s.id, feedback, by).await?;
    }
    Ok(())
}

/// Subscribes the company to Dialogporten's transmission events for its
/// a-melding dialogs, once, so the feedback arrives at `/webhooks/altinn`.
/// Best effort: without it the «Hent tilbakemelding» button still works,
/// so a failure is logged, never returned to the person who just sent.
async fn ensure_event_subscription(
    state: &AppState,
    rail: &crate::AmeldingRail,
    company_id: Uuid,
    orgnumber: &str,
    by: &str,
) {
    use regnmed_gov::altinn_events::{AMELDING_RESOURCE, TRANSMISSION_CREATED};
    let Some(endpoint) = &rail.events_endpoint else {
        return;
    };
    let result = async {
        if regnmed_db::amelding_submission::event_subscription(
            &state.pool,
            company_id,
            AMELDING_RESOURCE,
            TRANSMISSION_CREATED,
            endpoint,
        )
        .await?
        .is_some()
        {
            return Ok(());
        }
        let subscription = rail
            .events
            .subscribe_amelding(&rail.tokens, orgnumber, endpoint)
            .await?;
        regnmed_db::amelding_submission::record_event_subscription(
            &state.pool,
            company_id,
            subscription.id,
            AMELDING_RESOURCE,
            TRANSMISSION_CREATED,
            endpoint,
            by,
        )
        .await
    }
    .await;
    if let Err(e) = result {
        eprintln!("altinn events: could not subscribe company {company_id}: {e:#}");
    }
}

/// `POST /webhooks/altinn` — Altinn Events' webhook. Public: Altinn does not
/// authenticate or sign its deliveries, so NOTHING in the event is trusted.
/// It is a hint naming a dialog; the feedback is then fetched through our
/// own systembruker-authenticated client, and only for a submission we
/// delivered and are still waiting on. The answer is always 2xx — an
/// unknown dialog is not an error worth Altinn retrying, and a failed
/// fetch leaves the button as the fallback.
pub async fn altinn_event(
    State(state): State<AppState>,
    Json(event): Json<regnmed_gov::altinn_events::CloudEvent>,
) -> axum::http::StatusCode {
    use regnmed_gov::altinn_events::{AMELDING_RESOURCE, VALIDATE_SUBSCRIPTION};
    let ok = axum::http::StatusCode::OK;
    if event.kind == VALIDATE_SUBSCRIPTION {
        return ok;
    }
    let (Some(rail), Some(AMELDING_RESOURCE), Some(dialog)) = (
        state.amelding.clone(),
        event.resource.as_deref(),
        event.resourceinstance.as_deref(),
    ) else {
        return ok;
    };
    let waiting =
        match regnmed_db::amelding_submission::awaiting_feedback_for_dialog(&state.pool, dialog)
            .await
        {
            Ok(waiting) => waiting,
            Err(e) => {
                eprintln!("altinn events: looking up dialog {dialog}: {e:#}");
                return ok;
            }
        };
    for (company_id, s) in waiting {
        let result = async {
            let (_, status) =
                regnmed_db::amelding_submission::submission(&state.pool, company_id, s.id).await?;
            fetch_feedback(&state, &rail, company_id, &s, &status, "altinn-events").await
        }
        .await;
        if let Err(e) = result {
            eprintln!("altinn events: feedback for submission {}: {e:#}", s.id);
        }
    }
    ok
}

pub async fn amelding_feedback(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, _run_id, submission_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use regnmed_db::amelding_submission::SubmissionStatus as S;
    require(&state, person.person_id, company_id, Right::PayrollRun).await?;
    let (s, status) =
        regnmed_db::amelding_submission::submission(&state.pool, company_id, submission_id)
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if matches!(status, S::Delivered { .. }) {
        let Some(rail) = state.amelding.clone() else {
            return Err(ApiError::BadRequest(
                "innsending av a-melding er ikke satt opp på denne serveren (Maskinporten)".into(),
            ));
        };
        fetch_feedback(&state, &rail, company_id, &s, &status, person.display())
            .await
            .map_err(|e| ApiError::BadRequest(format!("kunne ikke hente tilbakemeldingen: {e}")))?;
    }
    let (s, status) =
        regnmed_db::amelding_submission::submission(&state.pool, company_id, submission_id).await?;
    Ok(Json(submission_json(&s, &status)))
}

#[derive(Deserialize)]
pub struct YearQuery {
    year: Option<i32>,
}

fn run_json(k: &regnmed_db::payroll::PayrollRun) -> serde_json::Value {
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
        "kjoringer": runs.iter().map(run_json).collect::<Vec<_>>()
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
        "kjoring": run_json(&run),
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
    let g = regnmed_db::payroll::hour_basis(&state.pool, company_id, employee_id, q.year, q.month)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "minutter": g.minutes,
        "timesats_ore": g.hourly_rate_ore,
        "belop_ore": g.amount_ore,
        "laast": g.locked,
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
