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
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

fn aksjonaer_json(a: &regnmed_db::aksjebok::Aksjonaer) -> serde_json::Value {
    json!({
        "id": a.id,
        "kind": a.kind,
        "navn": a.navn,
        // §4-5: birth date, not fødselsnummer.
        "fodselsdato": a.fodselsdato,
        "orgnr": a.orgnr,
        "utenlandsk_id": a.utenlandsk_id,
        "adresse": a.adresse,
        "postnummer": a.postnummer,
        "poststed": a.poststed,
        "landkode": a.landkode,
        "note": a.note,
        "antall_aksjer": a.antall_aksjer,
        "andel_bp": a.andel_bp,
    })
}

#[derive(Deserialize)]
pub struct BokQuery {
    /// The aksjeeierbok is a function of a date; today when unset.
    dato: Option<chrono::NaiveDate>,
}

pub async fn list_shareholders(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<BokQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::AksjebokLes).await?;
    let dato = q.dato.unwrap_or_else(|| chrono::Utc::now().date_naive());
    let bok = regnmed_db::aksjebok::aksjeeierbok(&state.pool, company_id, dato)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let totalt: i64 = bok.iter().map(|a| a.antall_aksjer).sum();
    Ok(Json(json!({
        "dato": dato,
        "totalt_antall_aksjer": totalt,
        "aksjonarer": bok.iter().map(aksjonaer_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateShareholderRequest {
    kind: String,
    navn: String,
    fodselsnummer: Option<String>,
    orgnr: Option<String>,
    utenlandsk_id: Option<String>,
    adresse: Option<String>,
    postnummer: Option<String>,
    poststed: Option<String>,
    landkode: Option<String>,
    note: Option<String>,
}

pub async fn create_shareholder(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateShareholderRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::AksjebokSkriv).await?;
    let id = regnmed_db::aksjebok::create_aksjonaer(
        &state.pool,
        company_id,
        &regnmed_db::aksjebok::NyAksjonaer {
            kind: request.kind,
            navn: request.navn,
            fodselsnummer: request.fodselsnummer,
            orgnr: request.orgnr,
            utenlandsk_id: request.utenlandsk_id,
            adresse: request.adresse,
            postnummer: request.postnummer,
            poststed: request.poststed,
            landkode: request.landkode,
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
    navn: String,
    adresse: Option<String>,
    postnummer: Option<String>,
    poststed: Option<String>,
    landkode: Option<String>,
}

pub async fn update_contact(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, shareholder_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ContactRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::AksjebokSkriv).await?;
    regnmed_db::aksjebok::update_aksjonaer_kontakt(
        &state.pool,
        company_id,
        shareholder_id,
        &request.navn,
        request.adresse.as_deref(),
        request.postnummer.as_deref(),
        request.poststed.as_deref(),
        request.landkode.as_deref(),
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
    krev(&state, person.person_id, company_id, Rett::AksjebokLes).await?;
    let types: Vec<_> = regnmed_core::aksjebok::ALLE
        .iter()
        .map(|t| {
            json!({
                "slug": t.slug(),
                "navn": t.navn(),
                "tilgang": t.er_tilgang(),
                "leverbar": t.kode().is_some(),
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
    krev(&state, person.person_id, company_id, Rett::AksjebokLes).await?;
    let events = regnmed_db::aksjebok::list_hendelser(&state.pool, company_id, q.year).await?;
    Ok(Json(json!({
        "hendelser": events.iter().map(|e| json!({
            "id": e.id,
            "shareholder_id": e.shareholder_id,
            "aksjonar": e.aksjonaer,
            "type": e.type_,
            "type_navn": e.type_navn,
            "dato": e.dato,
            "antall": e.antall,
            "belop_ore": e.belop_ore,
            "motpart": e.motpart,
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
    dato: chrono::NaiveDate,
    antall: i64,
    belop_ore: Option<i64>,
    /// The other side of a transfer, written in the same transaction.
    motpart_id: Option<Uuid>,
    motpart_type: Option<String>,
    note: Option<String>,
}

pub async fn create_event(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateEventRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::AksjebokSkriv).await?;
    let id = regnmed_db::aksjebok::record_hendelse(
        &state.pool,
        company_id,
        request.shareholder_id,
        &request.type_,
        request.dato,
        request.antall,
        request.belop_ore,
        request.motpart_id,
        request.motpart_type.as_deref(),
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
    krev(&state, person.person_id, company_id, Rett::AksjebokLes).await?;
    let rows = regnmed_db::aksjebok::list_utbytte(&state.pool, company_id, q.year).await?;
    Ok(Json(json!({
        "utbytte": rows.iter().map(|u| json!({
            "id": u.id,
            "besluttet_dato": u.besluttet_dato,
            "per_aksje_ore": u.per_aksje_ore,
            "totalt_ore": u.totalt_ore,
            "voucher_id": u.voucher_id,
            "note": u.note,
            "created_by": u.created_by,
        })).collect::<Vec<_>>()
    })))
}

#[derive(Deserialize)]
pub struct CreateDividendRequest {
    besluttet_dato: chrono::NaiveDate,
    per_aksje_ore: i64,
    note: Option<String>,
}

pub async fn create_dividend(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateDividendRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::AksjebokSkriv).await?;
    let vedtak = regnmed_db::aksjebok::create_utbytte(
        &state.pool,
        company_id,
        request.besluttet_dato,
        request.per_aksje_ore,
        request.note.as_deref(),
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "dividend_id": vedtak.id,
        "totalt_ore": vedtak.totalt_ore,
        "voucher_id": vedtak.voucher_id,
    })))
}

#[derive(Deserialize)]
pub struct OppgaveQuery {
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
pub async fn oppgave(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<OppgaveQuery>,
) -> Result<Response, ApiError> {
    krev(&state, person.person_id, company_id, Rett::AksjebokLes).await?;
    let sett = regnmed_db::aksjebok::bygg_oppgave(&state.pool, company_id, q.year)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Always rendered, but an error is only fatal when somebody actually
    // asks for the file. The preview must SHOW what is blocking the
    // filing — a blank page helps nobody understand why.
    let hoved = regnmed_core::aksjonaeroppgave::render_hovedskjema(&sett.hovedskjema);
    let under: Vec<_> = sett
        .underskjemaer
        .iter()
        .map(|(id, u)| {
            (
                *id,
                u,
                regnmed_core::aksjonaeroppgave::render_underskjema(u),
            )
        })
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
                "navn": u.navn,
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
            hindringer.push(format!("{}: {e}", u.navn));
        }
    }

    Ok(Json(json!({
        "leverbar": hindringer.is_empty(),
        "hindringer": hindringer,
        "inntektsar": q.year,
        "antall_aksjonarer": under.len(),
        "antall_aksjer": sett.hovedskjema.antall_aksjer.i_ar,
        "antall_aksjer_fjoraret": sett.hovedskjema.antall_aksjer.fjoraret,
        "aksjekapital_ore": sett.hovedskjema.aksjekapital.i_ar,
        "palydende_ore": sett.hovedskjema.palydende_ore.i_ar,
        "utbytte": sett.hovedskjema.utbytte.iter().map(|u| json!({
            "dato": u.dato,
            "per_aksje_ore": u.per_aksje_ore,
            "totalt_ore": u.totalt_ore,
        })).collect::<Vec<_>>(),
        "aksjonarer": sett.underskjemaer.iter().map(|(id, u)| json!({
            "shareholder_id": id,
            "navn": u.navn,
            "antall_fjoraret": u.antall_aksjer.fjoraret,
            "antall": u.antall_aksjer.i_ar,
            "antall_bevegelser": u.bevegelser.len(),
        })).collect::<Vec<_>>(),
    }))
    .into_response())
}
