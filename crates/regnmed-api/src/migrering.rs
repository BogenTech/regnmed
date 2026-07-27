//! Migreringsimport, filtier (docs/migration.md, #19):
//!
//! - POST /companies/{id}/import/contacts?kind=kunde|leverandor
//! - POST /companies/{id}/import/open-items?kind=&konto=&motkonto=&dato=&preview=
//!
//! Begge tar CSV-en rått i body (som bankimporten) — filen er
//! eksportert fra det gamle systemet og lastes opp som den er.
//! Åpne poster har `?preview=true`: da leses filen, partene slås opp
//! og saldoen sjekkes, men ingenting bokføres. Import krever admin,
//! som resten av migreringen.

use axum::Json;
use axum::extract::{Path, Query, State};
use regnmed_core::migreringcsv::{PartKind, parse_apne_poster, parse_kontakter};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Krav, krev};

fn kind_from(raw: Option<&str>) -> Result<PartKind, ApiError> {
    match raw.map(str::trim) {
        Some("kunde") => Ok(PartKind::Kunde),
        Some("leverandor") | Some("leverandør") => Ok(PartKind::Leverandor),
        _ => Err(ApiError::BadRequest(
            "kind må være 'kunde' eller 'leverandor'".into(),
        )),
    }
}

#[derive(Deserialize)]
pub struct ContactsQuery {
    kind: Option<String>,
}

pub async fn import_contacts(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ContactsQuery>,
    body: String,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Admin).await?;
    let kind = kind_from(query.kind.as_deref())?;
    let rader = parse_kontakter(&body, kind).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if rader.is_empty() {
        return Err(ApiError::BadRequest("filen inneholder ingen rader".into()));
    }
    let report = regnmed_db::import_contacts(&state.pool, company_id, &rader)
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({
        "lest": rader.len(),
        "opprettet": report.opprettet,
        "oppdatert": report.oppdatert,
        "warnings": report.advarsler,
    })))
}

#[derive(Deserialize)]
pub struct OpenItemsQuery {
    kind: Option<String>,
    /// Reskontrokontoen postene hører til (1500 / 2400 by default).
    konto: Option<String>,
    /// Motkontoen bilaget balanseres mot; 2050 (annen egenkapital) som
    /// standard, samme plugg som åpningsbalansen bruker.
    motkonto: Option<String>,
    dato: Option<chrono::NaiveDate>,
    preview: Option<bool>,
}

pub async fn import_open_items(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<OpenItemsQuery>,
    body: String,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Admin).await?;
    let kind = kind_from(query.kind.as_deref())?;
    let konto = query.konto.unwrap_or_else(|| {
        match kind {
            PartKind::Kunde => "1500",
            PartKind::Leverandor => "2400",
        }
        .to_string()
    });
    let rader = parse_apne_poster(&body).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if rader.is_empty() {
        return Err(ApiError::BadRequest(
            "filen inneholder ingen åpne poster".into(),
        ));
    }

    let plan = regnmed_db::plan_open_items(&state.pool, company_id, kind, &konto, &rader)
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    if query.preview.unwrap_or(false) {
        return Ok(Json(json!({
            "preview": true,
            "kind": kind.as_str(),
            "konto": konto,
            "antall": plan.antall,
            "sum_ore": plan.sum_ore,
            "konto_saldo_ore": plan.konto_saldo_ore,
            "kan_importeres": plan.konto_saldo_ore == 0,
            "nye_parter": plan.nye_parter,
            "warnings": plan.advarsler,
        })));
    }

    let dato = match query.dato {
        Some(d) => d,
        None => sqlx::query_scalar("select current_date")
            .fetch_one(&state.pool)
            .await
            .map_err(anyhow::Error::from)?,
    };
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let report = regnmed_db::import_open_items(
        &state.pool,
        company_id,
        kind,
        &konto,
        query.motkonto.as_deref().unwrap_or("2050"),
        dato,
        &rader,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({
        "voucher": format!("{}-{}", report.posted.fiscal_year, report.posted.voucher_number),
        "antall": report.antall,
        "sum_ore": report.sum_ore,
        "opprettede_parter": report.opprettede_parter,
        "warnings": report.advarsler,
    })))
}
