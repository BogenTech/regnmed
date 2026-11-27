//! «Godkjenn og send» for the mva-melding (docs/mva.md).
//!
//! - GET  /companies/{id}/vat-filings?year=&period=
//! - POST /companies/{id}/vat-filings                 store it, start the sign-in
//! - GET  /companies/{id}/vat-filings/{fid}
//! - POST /companies/{id}/vat-filings/{fid}/authorize a new sign-in for it
//! - POST /companies/{id}/vat-filings/{fid}/approve   finish the sign-in
//! - POST /companies/{id}/vat-filings/{fid}/abandon
//! - POST /companies/{id}/vat-filings/{fid}/close-unresolved  after checking Altinn
//! - GET  /companies/{id}/vat-filings/{fid}/documents/{type}
//!
//! The browser returning from ID-porten proves nothing on its own: the portal
//! posts the code here with its own bearer token, and only the person who
//! started the sign-in can finish it. That is what stops someone tricking a
//! colleague into approving a filing they did not start.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Duration, Utc};
use regnmed_core::hash::sha256;
use regnmed_core::secret::{open, seal};
use regnmed_db::vat_filing::{self, ApprovalOutcome, Approver, Event, Filing, Prepared};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};
use crate::herding::{Vis, file_response, safe_content_type};
use crate::reports::build_vat_return;
use crate::{AppState, IdportenRail};

/// How long a person has to complete the sign-in.
const LOGIN_MINUTES: i64 = 15;

fn rail(state: &AppState) -> Result<&IdportenRail, ApiError> {
    state
        .idporten
        .as_deref()
        .ok_or_else(|| ApiError::BadRequest("Innsending av mva-melding er ikke slått på.".into()))
}

/// 244 random bits, in the alphabet PKCE allows.
fn random() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn name(person: &AuthPerson) -> &str {
    person.name.as_deref().unwrap_or(&person.sub)
}

async fn view(state: &AppState, filing: &Filing, events: &[Event]) -> Result<Value, ApiError> {
    let documents = vat_filing::documents(&state.pool, filing.id).await?;
    let open_job = vat_filing::job_open(&state.pool, filing.company_id, filing.id).await?;
    let approved = vat_filing::approved_by(events);
    let problem = events
        .iter()
        .rev()
        .find(|e| e.detail.get("melding").is_some() || e.detail.get("avvik").is_some())
        .filter(|e| e.kind == vat_filing::status(events));
    let payment = vat_filing::document(&state.pool, filing.id, "betalingsinformasjon")
        .await?
        .and_then(|(bytes, _)| {
            regnmed_gov::vat_return::payment_info(&String::from_utf8_lossy(&bytes))
        });
    Ok(json!({
        "id": filing.id,
        "year": filing.year,
        "period": filing.period,
        "status": vat_filing::status(events),
        "assessed_kr": filing.assessed_kr,
        "requested_by": filing.requested_by,
        "created_at": filing.created_at.to_rfc3339(),
        "godkjent_av": vat_filing::approver_name(events),
        "godkjent_at": approved.map(|e| e.created_at.to_rfc3339()),
        "melding": problem.and_then(|e| e.detail["melding"].as_str()),
        "avvik": problem.and_then(|e| e.detail.get("avvik")).cloned().unwrap_or(json!([])),
        "mangler_altinn_rett": problem.is_some_and(|e| e.detail["mangler_altinn_rett"] == true),
        "dokumenter": documents,
        "betaling": payment,
        "henter_svar": vat_filing::fetches_answer(&state.pool, filing.id).await?,
        "jobb_aktiv": open_job,
        "kan_godkjennes": vat_filing::can_authorize(events, open_job),
        "kan_avbrytes": vat_filing::can_abandon(events, open_job),
        "kan_lukkes": vat_filing::can_close_unresolved(events, open_job),
        "kan_vaere_sendt": vat_filing::may_have_reached_altinn(events),
        "korrigerer": vat_filing::corrects(events),
        "lukking": vat_filing::closing(events).map(|e| json!({
            "funn": e.detail["funn"],
            "notat": e.detail["notat"],
            "av": e.detail["name"],
            "tidspunkt": e.created_at.to_rfc3339(),
        })),
    }))
}

async fn load(
    state: &AppState,
    company_id: Uuid,
    id: Uuid,
) -> Result<(Filing, Vec<Event>), ApiError> {
    let filing = vat_filing::get(&state.pool, company_id, id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let events = vat_filing::events(&state.pool, filing.id).await?;
    Ok((filing, events))
}

#[derive(Serialize, Deserialize)]
struct PendingLogin {
    verifier: String,
    nonce: String,
    started_at: chrono::DateTime<Utc>,
}

/// Starts a sign-in and answers where to send the person. The company and
/// filing ride in `state`, so the portal can find its way back.
async fn begin_login(
    state: &AppState,
    rail: &IdportenRail,
    filing: &Filing,
    person_id: Uuid,
) -> Result<String, ApiError> {
    let login_state = format!("{}.{}.{}", filing.company_id, filing.id, random());
    let pending = PendingLogin {
        verifier: random(),
        nonce: random(),
        started_at: Utc::now(),
    };
    let challenge = URL_SAFE_NO_PAD.encode(sha256(pending.verifier.as_bytes()));
    let sealed = seal(
        &rail.key,
        &serde_json::to_vec(&pending).map_err(anyhow::Error::from)?,
    )
    .map_err(anyhow::Error::from)?;
    vat_filing::save_login(
        &state.pool,
        filing.id,
        person_id,
        &sha256(login_state.as_bytes()),
        &sealed,
        Utc::now() + Duration::minutes(LOGIN_MINUTES),
    )
    .await?;
    rail.client
        .authorize_url(&login_state, &pending.nonce, &challenge)
        .await
        .map_err(|e| {
            eprintln!("ID-porten sign-in for filing {}: {e:#}", filing.id);
            ApiError::BadRequest("Får ikke kontakt med ID-porten. Prøv igjen om litt.".into())
        })
}

#[derive(Deserialize)]
pub struct ListQuery {
    year: i32,
    period: u8,
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ReportRead).await?;
    let filings =
        vat_filing::list(&state.pool, company_id, query.year, i32::from(query.period)).await?;
    let mut out = Vec::with_capacity(filings.len());
    for filing in &filings {
        let events = vat_filing::events(&state.pool, filing.id).await?;
        out.push(view(&state, filing, &events).await?);
    }
    Ok(Json(json!({
        "tilgjengelig": state.idporten.is_some(),
        "filings": out,
    })))
}

pub async fn detail(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, filing_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ReportRead).await?;
    let (filing, events) = load(&state, company_id, filing_id).await?;
    Ok(Json(view(&state, &filing, &events).await?))
}

#[derive(Deserialize)]
pub struct CreateRequest {
    year: i32,
    period: u8,
}

pub async fn create(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateRequest>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    require(&state, person.person_id, company_id, Right::VatReturnSubmit).await?;
    crate::terms::require_accepted(&state, company_id).await?;
    let rail = rail(&state)?;
    let built = build_vat_return(&state, company_id, request.year, request.period).await?;
    if built.scheme.end(built.message.period) >= Utc::now().date_naive() {
        return Err(ApiError::BadRequest(
            "Terminen er ikke over ennå, så mva-meldingen kan ikke sendes.".into(),
        ));
    }
    let prepared = vat_filing::prepare(
        &state.pool,
        company_id,
        request.year,
        i32::from(request.period),
        &built.xml,
        built.message.assessed_kr,
        person.person_id,
        name(&person),
    )
    .await?;
    let filing = match prepared {
        Prepared::Ready(filing) => filing,
        Prepared::Refused(why) => return Err(ApiError::BadRequest(why)),
    };
    let authorize_url = begin_login(&state, rail, &filing, person.person_id).await?;
    let events = vat_filing::events(&state.pool, filing.id).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "filing": view(&state, &filing, &events).await?,
            "authorize_url": authorize_url,
        })),
    ))
}

/// A new sign-in for a filing that needs one to go on.
pub async fn authorize(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, filing_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VatReturnSubmit).await?;
    let rail = rail(&state)?;
    let (filing, events) = load(&state, company_id, filing_id).await?;
    let open_job = vat_filing::job_open(&state.pool, company_id, filing_id).await?;
    if !vat_filing::can_authorize(&events, open_job) {
        return Err(ApiError::BadRequest(
            "Denne innsendingen kan ikke godkjennes nå.".into(),
        ));
    }
    let authorize_url = begin_login(&state, rail, &filing, person.person_id).await?;
    Ok(Json(json!({
        "filing": view(&state, &filing, &events).await?,
        "authorize_url": authorize_url,
    })))
}

#[derive(Deserialize)]
pub struct ApproveRequest {
    code: String,
    state: String,
}

pub async fn approve(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, filing_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ApproveRequest>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    require(&state, person.person_id, company_id, Right::VatReturnSubmit).await?;
    crate::terms::require_accepted(&state, company_id).await?;
    let rail = rail(&state)?;
    let (filing, events) = load(&state, company_id, filing_id).await?;
    let open_job = vat_filing::job_open(&state.pool, company_id, filing_id).await?;
    if !vat_filing::can_authorize(&events, open_job) {
        return Err(ApiError::BadRequest(
            "Denne innsendingen kan ikke godkjennes nå.".into(),
        ));
    }
    // Used up here, whatever happens next: a sign-in works once.
    let sealed = vat_filing::take_login(
        &state.pool,
        filing_id,
        person.person_id,
        &sha256(request.state.as_bytes()),
    )
    .await?
    .ok_or_else(|| {
        ApiError::BadRequest("Innloggingen er utløpt eller allerede brukt. Prøv igjen.".into())
    })?;
    let pending: PendingLogin =
        serde_json::from_slice(&open(&rail.key, &sealed).map_err(anyhow::Error::from)?)
            .map_err(anyhow::Error::from)?;

    let period = u8::try_from(filing.period).map_err(anyhow::Error::from)?;
    let (scheme, term) =
        crate::reports::scheme_period(&state, company_id, filing.year, period).await?;
    // What the person saw is what is sent. With an instance on record only
    // the worker can tell whether it has left, and it makes this check then.
    if vat_filing::instance_url(&events).is_none() {
        let built = build_vat_return(&state, company_id, filing.year, period).await?;
        if built.xml != filing.melding_xml {
            return Err(ApiError::BadRequest(
                "Regnskapet er endret siden du startet. Se over terminen og godkjenn på nytt."
                    .into(),
            ));
        }
    }

    let approval = rail
        .client
        .redeem(
            &request.code,
            &pending.verifier,
            &pending.nonce,
            pending.started_at,
        )
        .await
        .map_err(|e| {
            eprintln!("ID-porten approval for filing {filing_id}: {e:#}");
            ApiError::BadRequest("ID-porten bekreftet ikke innloggingen. Prøv igjen.".into())
        })?;
    let orgnumber: String = sqlx::query_scalar("select orgnr from company where id = $1")
        .bind(company_id)
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let approver = Approver {
        person_id: person.person_id,
        name: name(&person).to_string(),
        sub: approval.sub.clone(),
        acr: approval.acr.clone(),
        auth_time: approval.auth_time,
        envelope_xml: regnmed_core::vat_return::render_envelope(
            &orgnumber,
            term,
            scheme,
            name(&person),
        ),
    };
    let sealed_token =
        seal(&rail.key, approval.access_token.as_bytes()).map_err(anyhow::Error::from)?;
    let job_id = match vat_filing::approve(
        &state.pool,
        &filing,
        rail.client.issuer(),
        rail.client.client_id(),
        &approver,
        &sealed_token,
        Utc::now() + Duration::seconds(approval.expires_in),
    )
    .await?
    {
        ApprovalOutcome::Approved { job_id } => job_id,
        ApprovalOutcome::NotApprovable => {
            return Err(ApiError::BadRequest(
                "Denne innsendingen er endret mens du logget inn, så den ble ikke godkjent. \
                 Se status og prøv igjen."
                    .into(),
            ));
        }
        ApprovalOutcome::WrongIdentity => {
            return Err(ApiError::BadRequest(
                "Du logget inn i ID-porten som en annen person enn den som er knyttet til \
                 brukeren din i regnmed. Innsendingen er ikke godkjent. Logg inn med din \
                 egen eID."
                    .into(),
            ));
        }
    };
    crate::jobs::enqueue(&state, job_id).await;
    let events = vat_filing::events(&state.pool, filing_id).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "filing": view(&state, &filing, &events).await?, "job_id": job_id })),
    ))
}

pub async fn abandon(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, filing_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VatReturnSubmit).await?;
    let (filing, events) = load(&state, company_id, filing_id).await?;
    let open_job = vat_filing::job_open(&state.pool, company_id, filing_id).await?;
    if !vat_filing::can_abandon(&events, open_job) {
        return Err(ApiError::BadRequest(
            "Denne innsendingen kan ha nådd Skatteetaten og kan ikke avbrytes.".into(),
        ));
    }
    if !vat_filing::abandon(&state.pool, &filing, name(&person)).await? {
        return Err(ApiError::BadRequest(
            "Denne innsendingen kan ha nådd Skatteetaten og kan ikke avbrytes.".into(),
        ));
    }
    let events = vat_filing::events(&state.pool, filing_id).await?;
    Ok(Json(view(&state, &filing, &events).await?))
}

#[derive(Deserialize)]
pub struct CloseRequest {
    #[serde(rename = "notat")]
    note: String,
    #[serde(rename = "sjekket_altinn")]
    checked_altinn: bool,
    /// `levert` or `ikke_levert`: what the person found in Altinn.
    #[serde(rename = "funn")]
    finding: String,
}

/// A person has looked in Altinn and closes a filing nobody can settle by
/// machine. Their finding is recorded as theirs, never as a receipt.
pub async fn close_unresolved(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, filing_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CloseRequest>,
) -> Result<Json<Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VatReturnSubmit).await?;
    let note = request.note.trim();
    let finding = vat_filing::Finding::parse(&request.finding);
    let Some(finding) = finding
        .filter(|_| request.checked_altinn && !note.is_empty() && note.chars().count() <= 2000)
    else {
        return Err(ApiError::BadRequest(
            "Bekreft at du har sjekket Altinn, velg om meldingen er levert, og skriv hva du \
             fant (høyst 2000 tegn)."
                .into(),
        ));
    };
    let (filing, _) = load(&state, company_id, filing_id).await?;
    if !vat_filing::close_unresolved(
        &state.pool,
        &filing,
        person.person_id,
        name(&person),
        finding,
        note,
    )
    .await?
    {
        return Err(ApiError::BadRequest(
            "Bare en uavklart innsending uten aktiv jobb kan lukkes.".into(),
        ));
    }
    let events = vat_filing::events(&state.pool, filing_id).await?;
    Ok(Json(view(&state, &filing, &events).await?))
}

pub async fn document(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, filing_id, data_type)): Path<(Uuid, Uuid, String)>,
) -> Result<Response, ApiError> {
    require(&state, person.person_id, company_id, Right::ReportRead).await?;
    let (filing, _) = load(&state, company_id, filing_id).await?;
    let (bytes, content_type) = vat_filing::document(&state.pool, filing.id, &data_type)
        .await?
        .ok_or(ApiError::NotFound)?;
    let content_type = safe_content_type(&content_type);
    let extension = if content_type == "application/pdf" {
        "pdf"
    } else {
        "xml"
    };
    let filename = format!(
        "mva-{data_type}-{}-termin{}.{extension}",
        filing.year, filing.period
    );
    Ok(file_response(
        Vis::Attachment,
        &filename,
        content_type,
        bytes,
    ))
}
