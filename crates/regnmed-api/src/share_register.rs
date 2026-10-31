//! Aksjeeierbok and aksjonærregisteroppgave (docs/aksjonaer.md, #43):
//!
//! - GET/POST /companies/{id}/shareholders            the aksjeeierbok
//! - PUT  /companies/{id}/shareholders/{sid}/contact  contact details
//! - GET/POST /companies/{id}/share-events            events
//! - GET/POST /companies/{id}/dividends               dividend decisions
//! - GET  /companies/{id}/shareholders/transaction-types
//! - GET  /companies/{id}/reports/aksjonaeroppgave?year=&format=
//!
//! Reading is open to every access level — the aksjeeierbok is a
//! register anyone has a right to inspect under aksjeloven §4-5, and a
//! revisor must be able to read it. Recording events requires bokforing
//! or admin.
//!
//! **The fødselsnummer never leaves this layer** in a listing: the JSON
//! carries the birth date, which is what §4-5 asks for. The number goes
//! one way — in

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

fn shareholder_json(a: &regnmed_db::share_register::Shareholder) -> serde_json::Value {
    json!({
        "id": a.id,
        "kind": a.kind,
        "navn": a.name,
        // §4-5: birth date, not fødselsnummer.
        "fodselsdato": a.birth_date,
        "orgnr": a.orgnumber,
        "utenlandsk_id": a.foreign_id,
        "adresse": a.address,
        "postnummer": a.postal_code,
        "poststed": a.postal_city,
        "landkode": a.country_code,
        "note": a.note,
        "antall_aksjer": a.share_count,
        "andel_bp": a.andel_bp,
    })
}

#[derive(Deserialize)]
pub struct BookQuery {
    /// The aksjeeierbok is a function of a date; today when unset.
    #[serde(rename = "dato")]
    date: Option<chrono::NaiveDate>,
}

pub async fn list_shareholders(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<BookQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::ShareRegisterRead,
    )
    .await?;
    let date = q.date.unwrap_or_else(|| chrono::Utc::now().date_naive());
    let book = regnmed_db::share_register::aksjeeierbok(&state.pool, company_id, date)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let total: i64 = book.iter().map(|a| a.share_count).sum();
    Ok(Json(json!({
        "dato": date,
        "totalt_antall_aksjer": total,
        "aksjonarer": book.iter().map(shareholder_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateShareholderRequest {
    kind: String,
    #[serde(rename = "navn")]
    name: String,
    #[serde(rename = "fodselsnummer")]
    nin: Option<String>,
    #[serde(rename = "orgnr")]
    orgnumber: Option<String>,
    #[serde(rename = "utenlandsk_id")]
    foreign_id: Option<String>,
    #[serde(rename = "adresse")]
    address: Option<String>,
    #[serde(rename = "postnummer")]
    postal_code: Option<String>,
    #[serde(rename = "poststed")]
    postal_city: Option<String>,
    #[serde(rename = "landkode")]
    country_code: Option<String>,
    note: Option<String>,
}

pub async fn create_shareholder(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateShareholderRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::ShareRegisterWrite,
    )
    .await?;
    let id = regnmed_db::share_register::create_shareholder(
        &state.pool,
        company_id,
        &regnmed_db::share_register::NewShareholder {
            kind: request.kind,
            name: request.name,
            nin: request.nin,
            orgnumber: request.orgnumber,
            foreign_id: request.foreign_id,
            address: request.address,
            postal_code: request.postal_code,
            postal_city: request.postal_city,
            country_code: request.country_code,
            note: request.note,
        },
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "shareholder_id": id })))
}

#[derive(Deserialize)]
pub struct ContactRequest {
    #[serde(rename = "navn")]
    name: String,
    #[serde(rename = "adresse")]
    address: Option<String>,
    #[serde(rename = "postnummer")]
    postal_code: Option<String>,
    #[serde(rename = "poststed")]
    postal_city: Option<String>,
    #[serde(rename = "landkode")]
    country_code: Option<String>,
}

pub async fn update_contact(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, shareholder_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ContactRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::ShareRegisterWrite,
    )
    .await?;
    regnmed_db::share_register::update_shareholder_contact(
        &state.pool,
        company_id,
        shareholder_id,
        &request.name,
        request.address.as_deref(),
        request.postal_code.as_deref(),
        request.postal_city.as_deref(),
        request.country_code.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

/// The transaction types, so the portal never hardcodes the list — and
/// so the honest gap is visible in the API: `leverbar` says whether we
/// hold a verified RF-1086 code for it.
pub async fn transaction_types(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::ShareRegisterRead,
    )
    .await?;
    let types: Vec<_> = regnmed_core::share_register::ALL
        .iter()
        .map(|t| {
            json!({
                "slug": t.slug(),
                "navn": t.name(),
                "tilgang": t.is_acquisition(),
                "leverbar": t.code().is_some(),
            })
        })
        .collect();
    Ok(Json(json!({ "typer": types })))
}

#[derive(Deserialize)]
pub struct EventQuery {
    year: Option<i32>,
}

pub async fn list_events(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<EventQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::ShareRegisterRead,
    )
    .await?;
    let events =
        regnmed_db::share_register::list_hendelser(&state.pool, company_id, q.year).await?;
    Ok(Json(json!({
        "hendelser": events.iter().map(|e| json!({
            "id": e.id,
            "shareholder_id": e.shareholder_id,
            "aksjonar": e.shareholder,
            "type": e.type_,
            "type_navn": e.type_name,
            "dato": e.date,
            "antall": e.quantity,
            "belop_ore": e.amount_ore,
            "motpart": e.counterparty,
            "note": e.note,
            "created_by": e.created_by,
        })).collect::<Vec<_>>()
    })))
}

#[derive(Deserialize)]
pub struct CreateEventRequest {
    shareholder_id: Uuid,
    #[serde(rename = "type")]
    type_: String,
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
    #[serde(rename = "antall")]
    quantity: i64,
    #[serde(rename = "belop_ore")]
    amount_ore: Option<i64>,
    /// The other side of a transfer, written in the same transaction.
    #[serde(rename = "motpart_id")]
    counterparty_id: Option<Uuid>,
    #[serde(rename = "motpart_type")]
    counterparty_type: Option<String>,
    note: Option<String>,
}

pub async fn create_event(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateEventRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::ShareRegisterWrite,
    )
    .await?;
    let id = regnmed_db::share_register::record_hendelse(
        &state.pool,
        company_id,
        request.shareholder_id,
        &request.type_,
        request.date,
        request.quantity,
        request.amount_ore,
        request.counterparty_id,
        request.counterparty_type.as_deref(),
        request.note.as_deref(),
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "event_id": id })))
}

pub async fn list_dividends(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<EventQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::ShareRegisterRead,
    )
    .await?;
    let rows = regnmed_db::share_register::list_utbytte(&state.pool, company_id, q.year).await?;
    Ok(Json(json!({
        "utbytte": rows.iter().map(|u| json!({
            "id": u.id,
            "besluttet_dato": u.decided_date,
            "per_aksje_ore": u.per_share_ore,
            "totalt_ore": u.total_ore,
            "voucher_id": u.voucher_id,
            "note": u.note,
            "created_by": u.created_by,
        })).collect::<Vec<_>>()
    })))
}

#[derive(Deserialize)]
pub struct CreateDividendRequest {
    #[serde(rename = "besluttet_dato")]
    decided_date: chrono::NaiveDate,
    #[serde(rename = "per_aksje_ore")]
    per_share_ore: i64,
    note: Option<String>,
}

pub async fn create_dividend(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateDividendRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::ShareRegisterWrite,
    )
    .await?;
    let vedtak = regnmed_db::share_register::create_utbytte(
        &state.pool,
        company_id,
        request.decided_date,
        request.per_share_ore,
        request.note.as_deref(),
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "dividend_id": vedtak.id,
        "totalt_ore": vedtak.total_ore,
        "voucher_id": vedtak.voucher_id,
    })))
}

#[derive(Deserialize)]
pub struct ReturnQuery {
    year: i32,
    /// `json` (default) previews; `xml` downloads the filing itself.
    format: Option<String>,
}

/// The RF-1086 filing for one inntektsår.
///
/// The JSON form is a preview a human can check before anything is
/// filed; `format=xml` returns the hovedskjema and every underskjema,
/// each already validated against Skatteetatens XSD by our own tests.
///
/// Submission is NOT here: it needs the Maskinporten scope
/// `skatteetaten:innrapporteringaksjonaerregisteroppgave` plus an Altinn
/// systembruker, neither of which we hold (docs/gov.md). Rendering
/// without pretending to send is the honest half.
pub async fn tax_return(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<ReturnQuery>,
) -> Result<Response, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::ShareRegisterRead,
    )
    .await?;
    let sett = regnmed_db::share_register::build_return(&state.pool, company_id, q.year)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Always rendered, but an error is only fatal when somebody actually
    // asks for the file. The preview must SHOW what is blocking the
    // filing — a blank page helps nobody understand why.
    let hoved = regnmed_core::shareholder_return::render_main_form(&sett.main_form);
    let under: Vec<_> = sett
        .underskjemaer
        .iter()
        .map(|(id, u)| (*id, u, regnmed_core::shareholder_return::render_sub_form(u)))
        .collect();

    if q.format.as_deref() == Some("xml") {
        let hoved = hoved.map_err(|e| ApiError::BadRequest(e.to_string()))?;
        let mut skjemaer = Vec::new();
        for (id, u, xml) in &under {
            let xml = xml
                .as_ref()
                .map_err(|e| ApiError::BadRequest(e.to_string()))?;
            skjemaer.push(json!({
                "shareholder_id": id,
                "navn": u.name,
                "xml": xml,
            }));
        }
        return Ok(Json(json!({
            "inntektsar": q.year,
            "hovedskjema": hoved,
            "underskjemaer": skjemaer,
        }))
        .into_response());
    }

    let mut hindringer: Vec<String> = Vec::new();
    if let Err(e) = &hoved {
        hindringer.push(e.to_string());
    }
    for (_, u, xml) in &under {
        if let Err(e) = xml {
            hindringer.push(format!("{}: {e}", u.name));
        }
    }

    Ok(Json(json!({
        "leverbar": hindringer.is_empty(),
        "hindringer": hindringer,
        "inntektsar": q.year,
        "antall_aksjonarer": under.len(),
        "antall_aksjer": sett.main_form.share_count.this_year,
        "antall_aksjer_fjoraret": sett.main_form.share_count.last_year,
        "aksjekapital_ore": sett.main_form.share_capital.this_year,
        "palydende_ore": sett.main_form.nominal_value_ore.this_year,
        "utbytte": sett.main_form.dividend.iter().map(|u| json!({
            "dato": u.date,
            "per_aksje_ore": u.per_share_ore,
            "totalt_ore": u.total_ore,
        })).collect::<Vec<_>>(),
        "aksjonarer": sett.underskjemaer.iter().map(|(id, u)| json!({
            "shareholder_id": id,
            "navn": u.name,
            "antall_fjoraret": u.share_count.last_year,
            "antall": u.share_count.this_year,
            "antall_bevegelser": u.movements.len(),
        })).collect::<Vec<_>>(),
    }))
    .into_response())
}
