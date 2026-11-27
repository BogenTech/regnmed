//! Trekkpålegg registered by hand (docs/trekkpalegg.md §6, #339) and
//! fetched from Skatteetaten (§2, #334).
//!
//! - GET  /companies/{id}/payroll/garnishments                    orders + settings (LONN_LES)
//! - POST /companies/{id}/payroll/garnishments/fetch              fetch from Skatteetaten (LONN_SKRIV)
//! - POST /companies/{id}/payroll/garnishments                    register (LONN_SKRIV)
//! - POST /companies/{id}/payroll/garnishments/letters?filename=  raw letter body (LONN_SKRIV)
//! - GET  /companies/{id}/payroll/garnishments/letters/{lid}      the letter (LONN_LES)
//! - PUT  /companies/{id}/payroll/garnishments/{gid}              change: a new version (LONN_SKRIV)
//! - POST /companies/{id}/payroll/garnishments/{gid}/stop         stop: a new version (LONN_SKRIV)
//! - POST /companies/{id}/payroll/garnishments/{gid}/versions/{v}/attest
//!   (ATTESTERING_UTFOR + LONN_LES)
//! - PUT  /companies/{id}/payroll/garnishment-settings           account and skatt KID (LONN_SKRIV)
//!
//! Who owes what is sensitive (§3 point 4): every read needs LONN_LES.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::Response;
use regnmed_core::garnishment::{Draft, Kind, Size};
use regnmed_db::garnishment::{self as db, Author, VersionDraft};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};
use crate::herding::{Vis, file_response};

fn bad(e: anyhow::Error) -> ApiError {
    ApiError::BadRequest(e.to_string())
}

fn size_json(size: Size) -> serde_json::Value {
    match size {
        Size::AmountOre(a) => json!({ "belop_ore": a, "prosent_bp": null }),
        Size::PercentBp(p) => json!({ "belop_ore": null, "prosent_bp": p }),
    }
}

fn order_json(company_id: Uuid, o: &db::Order) -> serde_json::Value {
    json!({
        "id": o.id,
        "employee_id": o.employee_id,
        "navn": o.employee_name,
        "type": o.kind.slug(),
        "status": o.status(),
        "versjoner": o.versions.iter().map(|v| {
            let mut j = size_json(v.size);
            j.as_object_mut().expect("object").extend([
                ("versjon".into(), json!(v.version)),
                ("gjelder_fra".into(), json!(v.valid_from)),
                ("gjelder_til".into(), json!(v.valid_to)),
                ("kid".into(), json!(v.kid)),
                ("mottaker_orgnr".into(), json!(v.payee_orgnumber)),
                ("mottaker_konto".into(), json!(v.payee_account)),
                ("frist_dager".into(), json!(v.due_days)),
                ("brev_id".into(), json!(v.letter_id)),
                ("brev_filnavn".into(), json!(v.letter_filename)),
                ("brev_url".into(), json!(letter_url(company_id, v.letter_id))),
                ("stoppet".into(), json!(v.stopped)),
                ("krever_attestering".into(), json!(v.needs_attestation)),
                ("attestering".into(), json!(v.attestation)),
                ("attestert_av".into(), json!(v.attested_by)),
                ("notat".into(), json!(v.note)),
                ("registrert_av".into(), json!(v.created_by)),
                ("registrert".into(), json!(v.created_at)),
            ]);
            j
        }).collect::<Vec<_>>(),
    })
}

/// A fetched order: its newest stored version, as the run reads it.
fn fetched_json(f: &db::FetchedOrder, last: Option<&db::LastList>) -> serde_json::Value {
    let in_list = last.is_some_and(|l| l.listed.contains(&f.trekkid));
    let mut j = json!({
        "trekkid": f.trekkid,
        "trekkversjon": f.version,
        "employee_id": f.employee.as_ref().map(|e| e.0),
        "navn": f.employee.as_ref().map(|e| &e.1),
        "i_siste_liste": in_list,
        "hentet": f.fetched_at,
    });
    let extra = match &f.order {
        Err(e) => json!({ "feil": e }),
        Ok(o) => json!({
            "saksnummer": o.case_number,
            "status": if o.active { "aktiv" } else { "avsluttet" },
            "opprettet": o.created,
            "kid": o.kid,
            "mottaker_orgnr": o.payee,
            "mottaker_konto": o.account,
            "perioder": o.periods.iter().map(|p| {
                let mut period = match &p.size {
                    Ok(size) => size_json(*size),
                    Err(e) => json!({ "feil": e }),
                };
                period.as_object_mut().expect("object").extend([
                    ("fra".into(), json!(p.from)),
                    ("til".into(), json!(p.to)),
                ]);
                period
            }).collect::<Vec<_>>(),
        }),
    };
    j.as_object_mut()
        .expect("object")
        .extend(extra.as_object().expect("object").clone());
    j
}

/// «Hent trekkpålegg» (§2 step 1): the same fetch the daily job runs.
pub async fn fetch(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollWrite).await?;
    let Some(rail) = state.garnishment.clone() else {
        return Err(ApiError::BadRequest(
            "henting av trekkpålegg fra Skatteetaten er ikke satt opp her".into(),
        ));
    };
    crate::terms::require_accepted(&state, company_id).await?;
    let by = person.display();
    let summary = db::fetch(&state.pool, company_id, &rail, by)
        .await
        .map_err(bad)?;
    regnmed_db::schedule::ensure(
        &state.pool,
        company_id,
        regnmed_db::job::Kind::GarnishmentFetch,
        by,
    )
    .await?;
    Ok(Json(json!({
        "henting": summary.fetch_id,
        "antall": summary.listed,
        "nye_versjoner": summary.new_versions,
    })))
}

pub fn letter_url(company_id: Uuid, letter_id: Uuid) -> String {
    format!("/companies/{company_id}/payroll/garnishments/letters/{letter_id}")
}

fn settings_json(s: &db::Settings) -> serde_json::Value {
    json!({
        "gjelder_fra": s.valid_from,
        "konto": s.account_number,
        "skatt_kid": s.tax_kid,
        "skatt_konto": s.tax_account,
        "skatt_mottaker_orgnr": s.tax_payee_orgnumber,
        "satt_av": s.set_by,
        "registrert": s.created_at,
    })
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollRead).await?;
    let orders = db::list(&state.pool, company_id).await?;
    let settings = db::settings_history(&state.pool, company_id).await?;
    let mut conn = state.pool.acquire().await.map_err(anyhow::Error::from)?;
    let (fetched, last) = db::fetched(&mut conn, company_id).await?;
    drop(conn);
    let attempts = db::fetch_attempts(&state.pool, company_id).await?;
    Ok(Json(json!({
        "palegg": orders.iter().map(|o| order_json(company_id, o)).collect::<Vec<_>>(),
        "hentede": fetched.iter().map(|f| fetched_json(f, last.as_ref())).collect::<Vec<_>>(),
        "henting": {
            "tilgjengelig": state.garnishment.is_some(),
            "siste_vellykkede": last.as_ref().map(|l| l.at),
            "forsok": attempts.iter().map(|a| json!({
                "id": a.id,
                "av": a.requested_by,
                "tidspunkt": a.created_at,
                "ok": a.ok,
                "antall": a.listed,
                "feil": a.detail,
            })).collect::<Vec<_>>(),
        },
        "innstillinger": settings.first().map(settings_json),
        "innstillingshistorikk": settings.iter().map(settings_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct LetterQuery {
    filename: String,
}

pub async fn upload_letter(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<LetterQuery>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollWrite).await?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    let id = db::upload_letter(
        &state.pool,
        company_id,
        &q.filename,
        content_type,
        &body,
        person.display(),
    )
    .await
    .map_err(bad)?;
    Ok(Json(json!({ "brev_id": id })))
}

pub async fn letter(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, letter_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollRead).await?;
    let (filename, content_type, bytes) = db::letter(&state.pool, company_id, letter_id)
        .await
        .map_err(|_| ApiError::NotFound)?;
    Ok(file_response(Vis::Inline, &filename, &content_type, bytes))
}

#[derive(Deserialize)]
pub struct OrderRequest {
    /// Registration only; an order's employee never changes.
    employee_id: Option<Uuid>,
    /// samordnet, skatt or ovrige.
    #[serde(rename = "type")]
    kind: String,
    /// Monthly amount in øre, or ...
    #[serde(rename = "belop_ore")]
    amount_ore: Option<i64>,
    /// ... a percentage in basis points (two decimals).
    #[serde(rename = "prosent_bp")]
    percent_bp: Option<i64>,
    #[serde(rename = "gjelder_fra")]
    valid_from: chrono::NaiveDate,
    #[serde(rename = "gjelder_til")]
    valid_to: Option<chrono::NaiveDate>,
    kid: Option<String>,
    #[serde(rename = "mottaker_orgnr")]
    payee_orgnumber: Option<String>,
    #[serde(rename = "mottaker_konto")]
    payee_account: Option<String>,
    /// Øvrige only: the letter's deadline in days after the pay date.
    #[serde(rename = "frist_dager")]
    due_days: Option<i32>,
    /// From the letter upload. Required on registration.
    #[serde(rename = "brev_id")]
    letter_id: Option<Uuid>,
    #[serde(rename = "notat")]
    note: Option<String>,
}

impl OrderRequest {
    fn version(self) -> Result<VersionDraft, ApiError> {
        let kind = Kind::from_slug(&self.kind).ok_or_else(|| {
            ApiError::BadRequest("type må være samordnet, skatt eller ovrige".into())
        })?;
        let size = match (self.amount_ore, self.percent_bp) {
            (Some(a), None) => Size::AmountOre(a),
            (None, Some(p)) => Size::PercentBp(p),
            _ => {
                return Err(ApiError::BadRequest(
                    "oppgi enten beløp per måned eller prosent".into(),
                ));
            }
        };
        Ok(VersionDraft {
            draft: Draft {
                kind,
                size,
                valid_from: self.valid_from,
                valid_to: self.valid_to,
                kid: self.kid,
                payee_orgnumber: self.payee_orgnumber,
                payee_account: self.payee_account,
                due_days: self.due_days,
            },
            letter_id: self.letter_id,
            note: self.note,
        })
    }
}

pub async fn register(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<OrderRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollWrite).await?;
    let employee_id = request
        .employee_id
        .ok_or_else(|| ApiError::BadRequest("velg den ansatte".into()))?;
    let (id, version) = db::register(
        &state.pool,
        company_id,
        employee_id,
        request.version()?,
        &Author {
            person_id: person.person_id,
            name: person.display(),
        },
    )
    .await
    .map_err(bad)?;
    Ok(Json(json!({ "id": id, "versjon": version })))
}

pub async fn change(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, order_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<OrderRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollWrite).await?;
    let version = db::change(
        &state.pool,
        company_id,
        order_id,
        request.version()?,
        &Author {
            person_id: person.person_id,
            name: person.display(),
        },
    )
    .await
    .map_err(bad)?;
    Ok(Json(json!({ "id": order_id, "versjon": version })))
}

#[derive(Deserialize, Default)]
pub struct NoteRequest {
    #[serde(rename = "notat")]
    note: Option<String>,
}

pub async fn stop(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, order_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<NoteRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollWrite).await?;
    let version = db::stop(
        &state.pool,
        company_id,
        order_id,
        body.map(|b| b.0).unwrap_or_default().note,
        &Author {
            person_id: person.person_id,
            name: person.display(),
        },
    )
    .await
    .map_err(bad)?;
    Ok(Json(json!({ "id": order_id, "versjon": version })))
}

#[derive(Deserialize)]
pub struct AttestRequest {
    godkjent: bool,
    note: Option<String>,
}

pub async fn attest(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, order_id, version)): Path<(Uuid, Uuid, i32)>,
    Json(request): Json<AttestRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // The attestant must be able to read what they approve.
    require(&state, person.person_id, company_id, Right::PayrollRead).await?;
    require(&state, person.person_id, company_id, Right::ApprovalPerform).await?;
    db::attest(
        &state.pool,
        company_id,
        order_id,
        version,
        request.godkjent,
        request.note.as_deref(),
        &Author {
            person_id: person.person_id,
            name: person.display(),
        },
    )
    .await
    .map_err(bad)?;
    Ok(Json(json!({
        "decision": if request.godkjent { "godkjent" } else { "avvist" },
    })))
}

#[derive(Deserialize)]
pub struct SettingsRequest {
    #[serde(rename = "gjelder_fra")]
    valid_from: Option<chrono::NaiveDate>,
    #[serde(rename = "konto")]
    account_number: Option<String>,
    #[serde(rename = "skatt_kid")]
    tax_kid: Option<String>,
    #[serde(rename = "skatt_konto")]
    tax_account: Option<String>,
    #[serde(rename = "skatt_mottaker_orgnr")]
    tax_payee_orgnumber: Option<String>,
}

pub async fn set_settings(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(r): Json<SettingsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollWrite).await?;
    db::set_settings(
        &state.pool,
        company_id,
        &db::Settings {
            valid_from: r.valid_from,
            account_number: r.account_number,
            tax_kid: r.tax_kid,
            tax_account: r.tax_account,
            tax_payee_orgnumber: r.tax_payee_orgnumber,
            set_by: person.display().to_string(),
            created_at: None,
        },
    )
    .await
    .map_err(bad)?;
    Ok(Json(json!({ "ok": true })))
}
