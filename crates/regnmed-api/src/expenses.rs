//! Utlegg and kjøregodtgjørelse (docs/utlegg.md, #42):
//!
//! - GET  /companies/{id}/expenses?status=&limit=&offset=  list + tab counts
//! - GET  /companies/{id}/expenses/{eid}              one claim
//! - POST /companies/{id}/expenses/utlegg?filename=   raw receipt body
//! - POST /companies/{id}/expenses/kjoring            km claim
//! - GET  /companies/{id}/expenses/kjoring?dato=      the km-sats that day
//! - GET  /companies/{id}/expenses/{eid}/receipt
//! - POST /companies/{id}/expenses/{eid}/approve      posts + attaches
//! - POST /companies/{id}/expenses/{eid}/reject       note required
//! - POST /companies/{id}/expenses/{eid}/pay          mellomregning→bank
//!
//! Drafts (finish later, docs/utlegg.md):
//!
//! - POST /companies/{id}/expenses/receipts?filename=&person_id=  raw receipt → draft
//! - POST /companies/{id}/expenses                    JSON draft (kjøring, or a cost of a trip)
//! - PUT  /companies/{id}/expenses/{eid}              rewrite a draft
//! - PUT  /companies/{id}/expenses/{eid}/receipt?filename=  replace a draft's receipt
//! - POST /companies/{id}/expenses/{eid}/submit       → innsendt (content frozen)
//! - POST /companies/{id}/expenses/{eid}/discard      → forkastet
//! - GET  /companies/{id}/expenses/claimants          who a claim may be filed for
//! - GET  /companies/{id}/expenses/addresses?q=       address/company suggestions
//! - GET  /companies/{id}/expenses/route?from=&to=    driving distance + route line
//!
//! A draft is written by its claimant or by whoever entered it on their
//! behalf; filing for someone else takes UTLEGG_GODKJENN (the people who
//! handle claims), and the claimant must be a member of the company.
//!
//! Everyone with write access registers their OWN claims; approving,
//! rejecting and paying require bokforing or admin (four-eyes comes
//! with attestering, #47 — a one-person company must be able to do
//! everything today).

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};
use crate::herding::{Vis, file_response};

#[derive(Deserialize)]
pub struct UploadQuery {
    filename: String,
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
    #[serde(rename = "belop_ore")]
    amount_ore: Option<i64>,
    #[serde(rename = "beskrivelse", default)]
    description: String,
}

pub async fn create_expense(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<UploadQuery>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let id = regnmed_db::create_expense(
        &state.pool,
        company_id,
        person.person_id,
        query.date,
        &query.description,
        query
            .amount_ore
            .ok_or_else(|| ApiError::BadRequest("oppgi beløpet".into()))?,
        &query.filename,
        &content_type,
        &body,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "expense_id": id })))
}

#[derive(Deserialize)]
pub struct RunRequest {
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
    /// Route and purpose, e.g. "Oslo–Drammen t/r, kundemøte".
    #[serde(rename = "beskrivelse", default)]
    description: String,
    km: Option<i64>,
}

#[derive(Deserialize)]
pub struct RateQuery {
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
    /// `bil` when omitted.
    vehicle: Option<String>,
}

/// The satser a kjøring on `dato` would be paid with — so the form can
/// show the amount, and the trekkpliktig part, before anyone submits.
pub async fn mileage_rate(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<RateQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let rates = regnmed_db::trip_rates(
        &state.pool,
        query.date,
        query.vehicle.as_deref().unwrap_or("bil"),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "sats_ore_per_km": rates.rate,
        "trekkfri_ore_per_km": rates.tax_free,
        // None when the register has no supplement for the date.
        "passenger_rate_ore_per_km": rates.passenger.map(|p| p.0),
        "passenger_tax_free_ore_per_km": rates.passenger.map(|p| p.1),
    })))
}

pub async fn create_expense_run(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<RunRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let (id, amount_ore, trekkpliktig_ore) = regnmed_db::create_expense_run(
        &state.pool,
        company_id,
        person.person_id,
        request.date,
        &request.description,
        request
            .km
            .ok_or_else(|| ApiError::BadRequest("oppgi antall kilometer".into()))?,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "expense_id": id,
        "belop_ore": amount_ore,
        "trekkpliktig_ore": trekkpliktig_ore,
    })))
}

#[derive(Deserialize, Default)]
pub struct ListQuery {
    /// `apne` (innsendt + godkjent), `utkast`, `utbetalt`, `avvist`, … —
    /// empty = everything but drafts.
    status: Option<String>,
    /// The costs tied to this trip, drafts included.
    trip: Option<Uuid>,
    limit: Option<i64>,
    offset: Option<i64>,
}

fn expense_json(e: &regnmed_db::ExpenseRow) -> serde_json::Value {
    let mut v = json!({
        "expense_id": e.id,
        "person": e.person_name,
        "own": e.own,
        "kind": e.kind,
        "dato": e.date,
        "beskrivelse": e.description,
        "belop_ore": e.amount_ore,
        "km": e.km,
        "sats_ore_per_km": e.sats_ore_per_km,
        "trekkfri_ore": e.trekkfri_ore,
        "trekkpliktig_ore": e.trekkpliktig_ore,
        "receipt_filename": e.receipt_filename,
        "status": e.status,
        "avvist_note": e.avvist_note,
        "decided_by": e.decided_by,
        "decided_at": e.decided_at,
        "voucher": e.voucher,
        "utbetalt_voucher": e.paid_voucher,
        "utbetalt_at": e.utbetalt_at,
        "created_at": e.created_at,
        "registered_by": e.registered_by,
        "editable": e.editable,
        "entered_by_me": e.entered_by_me,
        "department": e.department,
        "project": e.project,
        "billable": e.billable,
        "route_from": e.route_from,
        "route_to": e.route_to,
        "round_trip": e.round_trip,
        "vehicle": e.vehicle,
        "passengers": e.passengers,
        "passenger_rate_ore_per_km": e.passenger_rate_ore_per_km,
        "trip_expense_id": e.trip_expense_id,
        "paid_by": e.paid_by,
        "rebilled_invoice_id": e.rebilled_invoice_id,
        "rebilled_as": e.rebilled_as,
    });
    // A trip's reiseregning; separate only because one json! this long
    // overflows the macro's recursion limit.
    v.as_object_mut().expect("an object").extend(
        json!({
            "departure_at": e.departure_at,
            "return_at": e.return_at,
            "destination": e.destination,
            "overnight": e.overnight,
            "over_15_km": e.over_15_km,
            "travel_days": e.travel_days,
            "rate_scheme": e.rate_scheme,
            "travel_lines": e.travel_lines,
            "company_card_id": e.company_card_id,
            "card_label": e.card_label,
            "card_account": e.card_account,
            "bank_transaction_id": e.bank_transaction_id,
        })
        .as_object()
        .expect("an object")
        .clone(),
    );
    v
}

#[derive(Deserialize)]
pub struct TravelRateQuery {
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
}

/// What a trip starting on `dato` is paid with: the company's choice of
/// rates, the domestic rates on both sides, and the state's table abroad
/// (the form's country picker).
pub async fn travel_rates(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<TravelRateQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let register = regnmed_db::load_rates(&state.pool).await?;
    let missing = |d: String| {
        ApiError::BadRequest(format!(
            "ingen sats «{d}» for {} i satsregisteret",
            query.date
        ))
    };
    let tax_free =
        regnmed_core::travel::TaxFreeRates::on(&register, query.date).map_err(missing)?;
    let st = regnmed_core::travel::StateRates::on(&register, query.date).map_err(missing)?;
    let scheme = regnmed_db::travel::travel_scheme(&state.pool, company_id).await?;
    let abroad = regnmed_db::travel::abroad_rates(&state.pool, query.date).await?;
    Ok(Json(json!({
        "scheme": scheme.slug(),
        "trekkfri": {
            "hotell": tax_free.hotel,
            "pensjonat": tax_free.pension,
            "privat": tax_free.private,
            "dag_6_12": tax_free.day_6_12,
            "dag_over_12": tax_free.day_over_12,
            "nattillegg": tax_free.night,
        },
        "statens": {
            "overnatting": st.overnight,
            "dag_6_12": st.day_6_12,
            "dag_over_12": st.day_over_12,
            "nattillegg": st.night,
        },
        "abroad": abroad.iter().map(|r| json!({
            "key": r.key(),
            "region": r.region,
            "country": r.country,
            "area": r.area,
            "amount_minor": r.amount_minor,
            "currency": r.currency,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct TravelPreviewRequest {
    departure_at: chrono::NaiveDateTime,
    return_at: chrono::NaiveDateTime,
    #[serde(default)]
    overnight: bool,
    #[serde(default)]
    travel_days: Vec<regnmed_db::travel::TravelDayDraft>,
}

/// The allowance a trip WOULD get, computed by the same code a
/// submission runs — the form shows the server's numbers, never its own.
/// A trip that gives nothing as entered answers 400 with the reason.
pub async fn travel_preview(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<TravelPreviewRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let scheme = regnmed_db::travel::travel_scheme(&state.pool, company_id).await?;
    let (allowance, lines) = regnmed_db::travel::compute(
        &state.pool,
        request.departure_at,
        request.return_at,
        request.overnight,
        &request.travel_days,
        scheme,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "scheme": scheme.slug(),
        "belop_ore": allowance.amount_ore,
        "trekkfri_ore": allowance.tax_free_ore,
        "trekkpliktig_ore": allowance.taxable_ore,
        "lines": lines,
    })))
}

#[derive(Deserialize)]
pub struct DayCountQuery {
    departure_at: chrono::NaiveDateTime,
    return_at: chrono::NaiveDateTime,
    #[serde(default)]
    overnight: bool,
}

/// How many døgn the form must ask about — the core's own count, so the
/// portal never cuts the trip differently from the submission.
pub async fn travel_days(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<DayCountQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let count =
        regnmed_core::travel::day_count(query.departure_at, query.return_at, query.overnight)
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "days": count })))
}

/// A viewer sees their own claims; with UTLEGG_LES_ALLE, the company's.
/// The scope is decided by the right, not by the role — then it moves
/// by itself when roles are recomposed (#60).
async fn scope(state: &AppState, person: Uuid, company_id: Uuid) -> Result<bool, ApiError> {
    let role = require(state, person, company_id, Right::ExpenseReadOwn).await?;
    Ok(!role.has(Right::ExpenseReadAll))
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let only_own = scope(&state, person.person_id, company_id).await?;
    let page = regnmed_db::list_expenses(
        &state.pool,
        company_id,
        person.person_id,
        only_own,
        &regnmed_db::ExpenseFilter {
            status: query.status,
            expense_id: None,
            trip_expense_id: query.trip,
            limit: query.limit.unwrap_or(200),
            offset: query.offset.unwrap_or(0),
        },
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "expenses": page.rows.iter().map(expense_json).collect::<Vec<_>>(),
        "total": page.total,
        "counts": {
            "utkast": page.counts.utkast,
            "innsendt": page.counts.innsendt,
            "godkjent": page.counts.godkjent,
            "utbetalt": page.counts.utbetalt,
            "avvist": page.counts.avvist,
        },
    })))
}

pub async fn get(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Someone else's claim, seen with only UTLEGG_LES_EGNE, answers as
    // though it does not exist — the same rule as the receipt.
    let only_own = scope(&state, person.person_id, company_id).await?;
    let page = regnmed_db::list_expenses(
        &state.pool,
        company_id,
        person.person_id,
        only_own,
        &regnmed_db::ExpenseFilter {
            expense_id: Some(expense_id),
            limit: 1,
            ..Default::default()
        },
    )
    .await?;
    let expense = page.rows.first().ok_or(ApiError::NotFound)?;
    Ok(Json(expense_json(expense)))
}

/// The receipt is a picture of something private. Without
/// UTLEGG_LES_ALLE you get only your own, and a DRAFT is nobody's but its
/// claimant's and whoever entered it — for everyone else it does not
/// exist, the same rule as the claim itself.
async fn readable_receipt(
    state: &AppState,
    person: &AuthPerson,
    company_id: Uuid,
    expense_id: Uuid,
) -> Result<(String, String, Vec<u8>), ApiError> {
    let role = require(state, person.person_id, company_id, Right::ExpenseReadOwn).await?;
    let (claimant, entered_by, status) =
        regnmed_db::expense_people(&state.pool, company_id, expense_id)
            .await
            .map_err(|_| ApiError::NotFound)?;
    let mine = claimant == person.person_id || entered_by == Some(person.person_id);
    if !mine && (status == "utkast" || !role.has(Right::ExpenseReadAll)) {
        return Err(ApiError::NotFound);
    }
    regnmed_db::expense_receipt(&state.pool, company_id, expense_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))
}

pub async fn receipt(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let (filename, content_type, content) =
        readable_receipt(&state, &person, company_id, expense_id).await?;
    // A receipt photo is shown in place, but only when we are willing to
    // assert its type — file_response downgrades anything else (#64).
    Ok(file_response(
        Vis::Inline,
        &filename,
        &content_type,
        content,
    ))
}

/// What the receipt itself says (docs/utlegg.md «Kvitteringen leses»):
/// date, amount and who was paid, each with the reason we believe it.
/// Read again on every call and never stored -- a better reader improves
/// old drafts too, and nothing here is a fact until the claimant saves it.
pub async fn suggestion(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (_, content_type, content) =
        readable_receipt(&state, &person, company_id, expense_id).await?;
    let f = crate::inbox::read_document(&state, company_id, &content_type, &content).await?;
    let mut reasons: Vec<serde_json::Value> = f
        .begrunnelser
        .iter()
        .filter_map(|(field, why)| {
            let field = match field.as_str() {
                "dato" => "date",
                "brutto" => "amount_ore",
                "mva" => "vat_ore",
                "orgnr" => "orgnumber",
                _ => return None,
            };
            Some(json!({ "field": field, "why": why }))
        })
        .collect();
    // Who was paid: the company's own supplier register, then the
    // Enhetsregisteret name for the orgnr, then the receipt's top line.
    let registry_name = match (&f.supplier_name, &f.orgnumber) {
        (None, Some(orgnumber)) => regnmed_gov::brreg::BrregClient::from_env()
            .entity(orgnumber)
            .await
            .ok()
            .flatten()
            .map(|e| e.name),
        _ => None,
    };
    let description = if let Some(name) = &f.supplier_name {
        Some((
            name.clone(),
            "leverandøren med dette orgnr i reskontroen".to_string(),
        ))
    } else if let Some(name) = registry_name {
        Some((
            name,
            "navnet i Enhetsregisteret for orgnr på kvitteringen".to_string(),
        ))
    } else {
        f.top_line.clone()
    };
    if let Some((_, why)) = &description {
        reasons.push(json!({ "field": "description", "why": why }));
    }
    // The inbox's warnings speak of the inbox's form: a missing supplier
    // and VAT on several rates are the approver's concern (the claim
    // carries one VAT code, chosen at approval), not the claimant's.
    let warnings: Vec<String> = f
        .warnings
        .into_iter()
        .filter(|w| !w.contains("leverandørreskontroen") && !w.contains("«Avansert»"))
        .collect();
    Ok(Json(json!({
        "source": f.source,
        "date": f.date.map(|d| d.to_string()),
        "amount_ore": f.gross_ore,
        "vat_ore": f.vat_ore,
        "orgnumber": f.orgnumber,
        "description": description.map(|(name, _)| name),
        "reasons": reasons,
        "warnings": warnings,
    })))
}

#[derive(Deserialize, Default)]
pub struct ApproveRequest {
    /// Kostnadskonto; defaults: utlegg 7790, kjøring 7100.
    #[serde(rename = "konto")]
    account: Option<String>,
    /// SAF-T code for inngående mva on utlegg (e.g. "1").
    #[serde(rename = "mva_kode")]
    vat_code: Option<String>,
    /// Inngående mva-konto, default 2710.
    #[serde(rename = "mva_konto")]
    vat_account: Option<String>,
    /// Mellomregning account, default 2910 (owed to employees).
    #[serde(rename = "motkonto")]
    contra_account: Option<String>,
}

pub async fn approve(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<ApproveRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseApprove).await?;
    let request = body.map(|Json(r)| r).unwrap_or_default();
    let kind: Option<String> =
        sqlx::query_scalar("select kind from expense where id = $1 and company_id = $2")
            .bind(expense_id)
            .bind(company_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(anyhow::Error::from)?;
    let default_account = match kind.as_deref() {
        Some("kjoring") => "7100",
        _ => "7790",
    };
    let decided_by = person.name.as_deref().unwrap_or(&person.sub);
    let approved = regnmed_db::approve_expense(
        &state.pool,
        company_id,
        expense_id,
        request.account.as_deref().unwrap_or(default_account),
        request.vat_code.as_deref(),
        request.vat_account.as_deref().unwrap_or("2710"),
        request.contra_account.as_deref().unwrap_or("2910"),
        decided_by,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "voucher": format!("{}-{}", approved.fiscal_year, approved.voucher_number),
        "warning": approved.warning,
    })))
}

#[derive(Deserialize)]
pub struct RejectRequest {
    note: String,
}

pub async fn reject(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RejectRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let access = require(&state, person.person_id, company_id, Right::ExpenseApprove).await?;
    // Someone's money is decided by a person (docs/utlegg.md «Agenten»).
    if access.is_machine() {
        return Err(ApiError::Forbidden(
            "et utleggskrav avgjøres av et menneske — en agent kan registrere og sende inn, \
             men ikke avvise",
        ));
    }
    let decided_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::reject_expense(
        &state.pool,
        company_id,
        expense_id,
        &request.note,
        decided_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "status": "avvist" })))
}

#[derive(Deserialize, Default)]
pub struct PayRequest {
    #[serde(rename = "dato")]
    date: Option<chrono::NaiveDate>,
    /// Bankkonto, default 1920.
    #[serde(rename = "konto")]
    account: Option<String>,
}

pub async fn pay(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<PayRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let access = require(&state, person.person_id, company_id, Right::ExpensePay).await?;
    if access.is_machine() {
        return Err(ApiError::Forbidden(
            "et utleggskrav utbetales av et menneske — en agent kan registrere og sende inn",
        ));
    }
    let request = body.map(|Json(r)| r).unwrap_or_default();
    let date = match request.date {
        Some(d) => d,
        None => sqlx::query_scalar("select current_date")
            .fetch_one(&state.pool)
            .await
            .map_err(anyhow::Error::from)?,
    };
    let paid_by = person.name.as_deref().unwrap_or(&person.sub);
    let paid = regnmed_db::pay_expense(
        &state.pool,
        company_id,
        expense_id,
        date,
        request.account.as_deref().unwrap_or("1920"),
        paid_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "voucher": format!("{}-{}", paid.fiscal_year, paid.voucher_number),
    })))
}

/// The fields of a draft, as the claim's own JSON names them — what a
/// client reads from GET is what it writes back.
#[derive(Deserialize, Default)]
pub struct DraftRequest {
    /// Create only: `utlegg`, `kjoring` or `reise`.
    kind: Option<String>,
    /// Create only: file for someone else (UTLEGG_GODKJENN).
    person_id: Option<Uuid>,
    /// Create only: the drive or trip this hangs under — a toll, a hotel
    /// bill, a drive on a journey.
    trip_expense_id: Option<Uuid>,
    #[serde(rename = "dato")]
    date: Option<chrono::NaiveDate>,
    #[serde(rename = "beskrivelse", default)]
    description: String,
    #[serde(rename = "belop_ore")]
    amount_ore: Option<i64>,
    km: Option<i64>,
    route_from: Option<String>,
    route_to: Option<String>,
    #[serde(default)]
    round_trip: bool,
    vehicle: Option<String>,
    #[serde(default)]
    passengers: Vec<String>,
    department: Option<String>,
    project: Option<String>,
    #[serde(default)]
    billable: bool,
    departure_at: Option<chrono::NaiveDateTime>,
    return_at: Option<chrono::NaiveDateTime>,
    destination: Option<String>,
    #[serde(default)]
    overnight: bool,
    #[serde(default)]
    over_15_km: bool,
    #[serde(default)]
    travel_days: Vec<regnmed_db::travel::TravelDayDraft>,
    company_card_id: Option<Uuid>,
    /// The card statement line this receipt documents. Kept by an update
    /// that still names a card, dropped by one that does not.
    bank_transaction_id: Option<Uuid>,
}

impl DraftRequest {
    fn draft(&self) -> regnmed_db::ExpenseDraft {
        regnmed_db::ExpenseDraft {
            date: self.date,
            description: self.description.clone(),
            amount_ore: self.amount_ore,
            km: self.km,
            route_from: self.route_from.clone(),
            route_to: self.route_to.clone(),
            round_trip: self.round_trip,
            vehicle: self.vehicle.clone(),
            passengers: self.passengers.clone(),
            department: self.department.clone(),
            project: self.project.clone(),
            billable: self.billable,
            departure_at: self.departure_at,
            return_at: self.return_at,
            destination: self.destination.clone(),
            overnight: self.overnight,
            over_15_km: self.over_15_km,
            travel_days: self.travel_days.clone(),
            company_card_id: self.company_card_id,
            bank_transaction_id: self.bank_transaction_id,
        }
    }
}

/// A shared card (no holder) and someone else's statement lines are for
/// whoever handles claims; a cardholder files for their own card.
async fn may_use_card(
    state: &AppState,
    person: &AuthPerson,
    company_id: Uuid,
    card_id: Option<Uuid>,
    line: Option<Uuid>,
) -> Result<(), ApiError> {
    let card_id = match (card_id, line) {
        (_, Some(line)) => Some(
            regnmed_db::company_card::card_line(&state.pool, company_id, line, None)
                .await
                .map_err(|e| ApiError::BadRequest(e.to_string()))?
                .card_id,
        ),
        (card, None) => card,
    };
    let Some(card_id) = card_id else {
        return Ok(());
    };
    let card = regnmed_db::company_card::get_card(&state.pool, company_id, card_id)
        .await
        .map_err(|_| ApiError::BadRequest("fant ikke kortet".into()))?;
    if card.holder_person_id != Some(person.person_id) {
        require(state, person.person_id, company_id, Right::ExpenseApprove).await?;
    }
    Ok(())
}

/// Whose claim a new draft is: the caller's own, or — for someone who
/// handles claims — a member's.
async fn claimant(
    state: &AppState,
    person: &AuthPerson,
    company_id: Uuid,
    asked: Option<Uuid>,
) -> Result<Uuid, ApiError> {
    require(state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    match asked {
        None => Ok(person.person_id),
        Some(id) if id == person.person_id => Ok(id),
        Some(id) => {
            require(state, person.person_id, company_id, Right::ExpenseApprove).await?;
            if !regnmed_db::is_claimant(&state.pool, company_id, id).await? {
                return Err(ApiError::BadRequest(
                    "personen er ikke medlem av selskapet".into(),
                ));
            }
            Ok(id)
        }
    }
}

/// A draft is written by its claimant or by whoever entered it; to
/// anyone else it does not exist.
async fn may_write(
    state: &AppState,
    person: &AuthPerson,
    company_id: Uuid,
    expense_id: Uuid,
) -> Result<(), ApiError> {
    require(state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let (claimant, entered_by, _) = regnmed_db::expense_people(&state.pool, company_id, expense_id)
        .await
        .map_err(|_| ApiError::NotFound)?;
    if person.person_id != claimant && entered_by != Some(person.person_id) {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

fn content_type(headers: &axum::http::HeaderMap) -> String {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string()
}

#[derive(Deserialize)]
pub struct ReceiptQuery {
    filename: String,
    person_id: Option<Uuid>,
    /// The card statement line the receipt documents: the draft takes the
    /// line's card, date, amount and text, and belongs to the cardholder.
    bank_transaction_id: Option<Uuid>,
}

/// One receipt → one draft: the photo is saved at once, the details
/// come later. Many receipts are many calls.
pub async fn create_receipt_draft(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ReceiptQuery>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut draft = regnmed_db::ExpenseDraft::default();
    let claimant = match query.bank_transaction_id {
        Some(line_id) => {
            require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
            may_use_card(&state, &person, company_id, None, Some(line_id)).await?;
            let line = regnmed_db::company_card::card_line(&state.pool, company_id, line_id, None)
                .await
                .map_err(|e| ApiError::BadRequest(e.to_string()))?;
            draft.date = Some(line.date);
            draft.amount_ore = Some(line.amount_ore);
            draft.description = line.description.clone();
            draft.bank_transaction_id = Some(line_id);
            line.holder_person_id.unwrap_or(person.person_id)
        }
        None => claimant(&state, &person, company_id, query.person_id).await?,
    };
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let id = regnmed_db::create_draft(
        &state.pool,
        company_id,
        claimant,
        person.person_id,
        "utlegg",
        None,
        &draft,
        Some(regnmed_db::Receipt {
            filename: &query.filename,
            content_type: &content_type(&headers),
            content: &body,
        }),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "expense_id": id })))
}

pub async fn create_draft(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<DraftRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // A cost of a trip belongs to the trip's claimant — also when whoever
    // entered the trip on their behalf adds the toll.
    let claimant = match (request.trip_expense_id, request.person_id) {
        (Some(trip), None) => {
            may_write(&state, &person, company_id, trip).await?;
            regnmed_db::expense_people(&state.pool, company_id, trip)
                .await
                .map_err(|_| ApiError::NotFound)?
                .0
        }
        (trip, asked) => {
            let claimant = claimant(&state, &person, company_id, asked).await?;
            if let Some(trip) = trip {
                may_write(&state, &person, company_id, trip).await?;
            }
            claimant
        }
    };
    may_use_card(
        &state,
        &person,
        company_id,
        request.company_card_id,
        request.bank_transaction_id,
    )
    .await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let id = regnmed_db::create_draft(
        &state.pool,
        company_id,
        claimant,
        person.person_id,
        request.kind.as_deref().unwrap_or("utlegg"),
        request.trip_expense_id,
        &request.draft(),
        None,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "expense_id": id })))
}

pub async fn update_draft(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<DraftRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    may_write(&state, &person, company_id, expense_id).await?;
    may_use_card(&state, &person, company_id, request.company_card_id, None).await?;
    regnmed_db::update_draft(&state.pool, company_id, expense_id, &request.draft())
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ReplaceReceiptQuery {
    filename: String,
}

pub async fn replace_receipt(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<ReplaceReceiptQuery>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    may_write(&state, &person, company_id, expense_id).await?;
    regnmed_db::replace_draft_receipt(
        &state.pool,
        company_id,
        expense_id,
        regnmed_db::Receipt {
            filename: &query.filename,
            content_type: &content_type(&headers),
            content: &body,
        },
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn submit(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    may_write(&state, &person, company_id, expense_id).await?;
    let done = regnmed_db::submit_expense(&state.pool, company_id, expense_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "belop_ore": done.amount_ore,
        "trekkpliktig_ore": done.taxable_ore,
        "costs_submitted": done.costs_submitted,
        "costs_waiting": done.costs_waiting,
    })))
}

pub async fn discard(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    may_write(&state, &person, company_id, expense_id).await?;
    regnmed_db::discard_expense(&state.pool, company_id, expense_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "status": "forkastet" })))
}

/// The people a claim may be filed for — for those who handle claims.
pub async fn claimants(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseApprove).await?;
    let members = regnmed_db::company_members(&state.pool, company_id).await?;
    let mut seen = std::collections::HashSet::new();
    Ok(Json(json!({
        "people": members
            .iter()
            .filter(|m| seen.insert(m.person_id))
            .map(|m| json!({ "person_id": m.person_id, "name": m.name }))
            .collect::<Vec<_>>(),
    })))
}

/// Approved billable claims not yet rebilled — what the new-invoice form
/// offers next to the unbilled hours (docs/utlegg.md «Viderefakturering»).
pub async fn billable(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseReadAll).await?;
    let claims = regnmed_db::billable_expenses(&state.pool, company_id).await?;
    Ok(Json(json!({
        "expenses": claims.iter().map(|c| json!({
            "expense_id": c.expense_id,
            "kind": c.kind,
            "date": c.date.to_string(),
            "description": c.description,
            "claimant": c.claimant,
            "amount_ore": c.amount_ore,
            "cost_ore": c.cost_ore,
            "project": c.project,
            "party_no": c.party_no,
            "party_name": c.party_name,
            "can_disburse": c.can_disburse,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct AddressQuery {
    q: String,
}

/// Suggestions for the trip form's address fields, three sources at
/// once: the company's own customers and suppliers (name → the address
/// on file), companies by name in Enhetsregisteret (their business
/// address) and Kartverket's address register — the external two
/// through us, so the CSP stays `connect-src 'self'`. Never an error to
/// the user: a source that fails is logged and contributes nothing.
pub async fn addresses(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<AddressQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let role = require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let text = query.q.trim();
    let mut out: Vec<serde_json::Value> =
        regnmed_db::own_addresses(&state.pool, company_id, person.person_id, text)
            .await?
            .into_iter()
            .map(|address| json!({ "name": null, "address": address }))
            .collect();
    // Too short to ask anyone else: your own recent addresses only.
    if text.chars().count() < 3 {
        return Ok(Json(json!({ "addresses": out })));
    }
    // The customer and supplier register is RESKONTRO_LES territory; an
    // employee filing a trip gets Enhetsregisteret and Kartverket only.
    let may_see_parties = role.has(Right::SubledgerRead);
    fn or_nothing<T>(source: &str, result: anyhow::Result<Vec<T>>) -> Vec<T> {
        result.unwrap_or_else(|e| {
            eprintln!("address suggestions from {source} failed: {e:#}");
            Vec::new()
        })
    }
    let brreg = regnmed_gov::brreg::BrregClient::from_env();
    let kartverket = regnmed_gov::addresses::AddressClient::from_env();
    let parties = async {
        if may_see_parties {
            regnmed_db::party_addresses(&state.pool, company_id, text).await
        } else {
            Ok(Vec::new())
        }
    };
    let (parties, companies, places) = tokio::join!(
        parties,
        brreg.search_by_name(text, 3),
        kartverket.search(text),
    );
    for (name, address) in or_nothing("parties", parties) {
        out.push(json!({ "name": name, "address": address }));
    }
    for entity in or_nothing("Enhetsregisteret", companies) {
        if let Some(address) = entity
            .forretningsadresse
            .as_ref()
            .and_then(|a| a.one_line())
        {
            out.push(json!({ "name": entity.name, "address": address }));
        }
    }
    for address in or_nothing("Kartverket", places) {
        out.push(json!({ "name": null, "address": address }));
    }
    Ok(Json(json!({ "addresses": out })))
}

#[derive(Deserialize)]
pub struct RouteQuery {
    from: String,
    to: String,
}

/// The driving distance between two addresses and the line to draw on a
/// map: Kartverket finds the points, OpenRouteService the route (both
/// through us). `available: false` when no routing key is configured —
/// the form then asks for the kilometres as it always has. A suggestion,
/// never the claim: the claimant states the km.
pub async fn route(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<RouteQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseReadOwn).await?;
    let Some(router) = regnmed_gov::routing::RouteClient::from_env() else {
        return Ok(Json(json!({ "available": false })));
    };
    // Every view of a trip asks again, and the routing quota is per day:
    // the same pair of addresses is answered from memory for a day.
    // shortcut: per replica and cleared wholesale at 1000 entries; move
    // to the shared cache if trips ever run into the quota.
    type Cache = std::sync::Mutex<
        std::collections::HashMap<(String, String), (std::time::Instant, serde_json::Value)>,
    >;
    static ROUTES: std::sync::LazyLock<Cache> = std::sync::LazyLock::new(Default::default);
    let key = (query.from.trim().to_string(), query.to.trim().to_string());
    if let Some((at, hit)) = ROUTES.lock().unwrap().get(&key)
        && at.elapsed() < std::time::Duration::from_secs(24 * 3600)
    {
        return Ok(Json(hit.clone()));
    }
    let places = regnmed_gov::addresses::AddressClient::from_env();
    let (from, to) = tokio::join!(places.locate(&query.from), places.locate(&query.to));
    let unreachable = |e: anyhow::Error| {
        eprintln!("route lookup failed: {e:#}");
        ApiError::BadRequest("ruten kan ikke beregnes akkurat nå — skriv kilometer selv".into())
    };
    let not_found =
        |text: &str| ApiError::BadRequest(format!("fant ikke adressen «{text}» på kartet"));
    let from = from
        .map_err(unreachable)?
        .ok_or_else(|| not_found(&query.from))?;
    let to = to
        .map_err(unreachable)?
        .ok_or_else(|| not_found(&query.to))?;
    let route = router.drive(from, to).await.map_err(unreachable)?;
    let answer = json!({
        "available": true,
        "km": (route.metres / 1000.0).round() as i64,
        "metres": route.metres,
        "line": route.line,
        "from": [from.lat, from.lon],
        "to": [to.lat, to.lon],
    });
    let mut cache = ROUTES.lock().unwrap();
    if cache.len() >= 1000 {
        cache.clear();
    }
    cache.insert(key, (std::time::Instant::now(), answer.clone()));
    Ok(Json(answer))
}

#[derive(Deserialize)]
pub struct FromInboxRequest {
    /// Who paid it and gets the money back.
    person_id: Uuid,
}

/// A receipt in the innboks that someone paid with their own money
/// becomes their DRAFT claim (docs/utlegg.md «Agenten»). Needs the right
/// to decide innboks documents and to file claims for others.
pub async fn from_inbox(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, document_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<FromInboxRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherPost).await?;
    let claimant = claimant(&state, &person, company_id, Some(request.person_id)).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let id = regnmed_db::expense_from_inbox(
        &state.pool,
        company_id,
        document_id,
        claimant,
        person.person_id,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "expense_id": id })))
}
