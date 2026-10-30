//! Migration import, file tier (docs/migration.md, #19):
//!
//! - POST /companies/{id}/import/contacts?kind=kunde|leverandor
//! - POST /companies/{id}/import/open-items?kind=&konto=&motkonto=&dato=&preview=
//!
//! Both take the CSV raw in the body (like the bank import) — the file is
//! exported from the old system and uploaded as-is. Open items support
//! `?preview=true`: the file is read, the parties resolved and the
//! balance checked, but nothing is posted. Import requires admin, like
//! the rest of the migration.

use axum::Json;
use axum::extract::{Path, Query, State};
use regnmed_core::migration_csv::{PartyKind, parse_contacts, parse_open_items};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

fn kind_from(raw: Option<&str>) -> Result<PartyKind, ApiError> {
    match raw.map(str::trim) {
        Some("kunde") => Ok(PartyKind::Customer),
        Some("leverandor") | Some("leverandør") => Ok(PartyKind::Supplier),
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
    krev(&state, person.person_id, company_id, Rett::MigreringAdmin).await?;
    let kind = kind_from(query.kind.as_deref())?;
    let rader = parse_contacts(&body, kind).map_err(|e| ApiError::BadRequest(e.to_string()))?;
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
    /// The reskontro account the items belong to (1500 / 2400 by default).
    #[serde(rename = "konto")]
    account: Option<String>,
    /// The contra account the bilag balances against; 2050 (annen
    /// egenkapital) by default, the same plug the opening balance uses.
    #[serde(rename = "motkonto")]
    contra_account: Option<String>,
    #[serde(rename = "dato")]
    date: Option<chrono::NaiveDate>,
    preview: Option<bool>,
}

pub async fn import_open_items(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<OpenItemsQuery>,
    body: String,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MigreringAdmin).await?;
    let kind = kind_from(query.kind.as_deref())?;
    let account = query.account.unwrap_or_else(|| {
        match kind {
            PartyKind::Customer => "1500",
            PartyKind::Supplier => "2400",
        }
        .to_string()
    });
    let rader = parse_open_items(&body).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if rader.is_empty() {
        return Err(ApiError::BadRequest(
            "filen inneholder ingen åpne poster".into(),
        ));
    }

    let plan = regnmed_db::plan_open_items(&state.pool, company_id, kind, &account, &rader)
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    if query.preview.unwrap_or(false) {
        return Ok(Json(json!({
            "preview": true,
            "kind": kind.as_str(),
            "konto": account,
            "antall": plan.antall,
            "sum_ore": plan.sum_ore,
            "konto_saldo_ore": plan.account_balance_ore,
            "kan_importeres": plan.account_balance_ore == 0,
            "nye_parter": plan.nye_parter,
            "warnings": plan.advarsler,
        })));
    }

    let date = match query.date {
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
        &account,
        query.contra_account.as_deref().unwrap_or("2050"),
        date,
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
