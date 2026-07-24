//! Produktregister og enkelt varelager (docs/produkter.md, #39):
//!
//! - GET/POST /companies/{id}/products             register
//! - PUT  /companies/{id}/products/{nummer}        edit (nummer immutable)
//! - GET  /companies/{id}/inventory                lagerstatus/telleliste
//! - GET  /companies/{id}/inventory/movements?produkt=NN
//! - POST /companies/{id}/inventory/movements      kjøp/justering
//! - POST /companies/{id}/inventory/count          varetelling (+ bilag)
//!
//! Also home of the shared document-line request: faktura, tilbud/ordre
//! and repeterende maler all take the same line shape, where `produkt`
//! fills every field the caller leaves out — copied at issue, so
//! register edits never touch existing documents.

use axum::Json;
use axum::extract::{Path, Query, State};
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
            "read-only access — produkter require bokforing",
        ));
    }
    Ok(())
}

/// One document line: free text (description + unit_price_ore) or a
/// product reference (`produkt`) whose register values fill whatever
/// the caller leaves out. Shared by faktura, tilbud/ordre and maler.
#[derive(Deserialize)]
pub struct DocLineRequest {
    pub produkt: Option<String>,
    pub description: Option<String>,
    /// Revenue account; defaults to the product's konto, else 3000.
    pub account: Option<String>,
    /// Thousandths (2,5 stk = 2500); defaults to 1000.
    pub quantity_milli: Option<i64>,
    pub unit_price_ore: Option<i64>,
    pub vat_code: Option<String>,
    pub avdeling: Option<String>,
    pub prosjekt: Option<String>,
}

pub async fn resolve_lines(
    state: &AppState,
    company_id: Uuid,
    lines: Vec<DocLineRequest>,
) -> Result<Vec<regnmed_db::InvoiceLineDraft>, ApiError> {
    let mut resolved = Vec::with_capacity(lines.len());
    for line in lines {
        let draft = regnmed_db::resolve_product_line(
            &state.pool,
            company_id,
            regnmed_db::ProductLineSpec {
                produkt: line.produkt,
                description: line.description,
                account: line.account,
                quantity_milli: line.quantity_milli.unwrap_or(1000),
                unit_price_ore: line.unit_price_ore,
                vat_code: line.vat_code,
                avdeling: line.avdeling,
                prosjekt: line.prosjekt,
            },
        )
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
        resolved.push(draft);
    }
    Ok(resolved)
}

#[derive(Deserialize)]
pub struct CreateProductRequest {
    nummer: String,
    navn: String,
    salgspris_ore: i64,
    vat_code: Option<String>,
    /// Defaults to 3000.
    konto: Option<String>,
    #[serde(default)]
    lagerfort: bool,
}

pub async fn create(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateProductRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let id = regnmed_db::create_product(
        &state.pool,
        company_id,
        &regnmed_db::ProductDraft {
            nummer: request.nummer,
            navn: request.navn,
            salgspris_ore: request.salgspris_ore,
            vat_code: request.vat_code,
            konto: request.konto.unwrap_or_else(|| "3000".into()),
            lagerfort: request.lagerfort,
        },
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "product_id": id })))
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let products = regnmed_db::list_products(&state.pool, company_id).await?;
    Ok(Json(json!({
        "products": products.iter().map(|p| json!({
            "nummer": p.nummer,
            "navn": p.navn,
            "salgspris_ore": p.salgspris_ore,
            "vat_code": p.vat_code,
            "konto": p.konto,
            "aktiv": p.aktiv,
            "lagerfort": p.lagerfort,
        })).collect::<Vec<_>>(),
    })))
}

/// `double_option`: distinguish an absent field (keep) from an explicit
/// null (clear) — same helper shape as invoice_template's slutt_dato.
fn double_option<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(de)?))
}

#[derive(Deserialize)]
pub struct UpdateProductRequest {
    navn: Option<String>,
    salgspris_ore: Option<i64>,
    #[serde(default, deserialize_with = "double_option")]
    vat_code: Option<Option<String>>,
    konto: Option<String>,
    aktiv: Option<bool>,
    lagerfort: Option<bool>,
}

pub async fn update(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, nummer)): Path<(Uuid, String)>,
    Json(request): Json<UpdateProductRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    regnmed_db::update_product(
        &state.pool,
        company_id,
        &nummer,
        request.navn.as_deref(),
        request.salgspris_ore,
        request.vat_code.as_ref().map(|v| v.as_deref()),
        request.konto.as_deref(),
        request.aktiv,
        request.lagerfort,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "updated": true })))
}

pub async fn inventory(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let rows = regnmed_db::inventory_status(&state.pool, company_id).await?;
    Ok(Json(json!({
        "inventory": rows.iter().map(|r| json!({
            "nummer": r.nummer,
            "navn": r.navn,
            "antall_milli": r.antall_milli,
            "verdi_ore": r.verdi_ore,
            "gjennomsnitt_ore": r.gjennomsnitt_ore,
        })).collect::<Vec<_>>(),
        "verdi_ore": rows.iter().map(|r| r.verdi_ore).sum::<i64>(),
    })))
}

#[derive(Deserialize)]
pub struct MovementsQuery {
    produkt: String,
}

pub async fn movements(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<MovementsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let rows = regnmed_db::list_movements(&state.pool, company_id, &query.produkt).await?;
    Ok(Json(json!({
        "movements": rows.iter().map(|m| json!({
            "dato": m.dato.to_string(),
            "kind": m.kind,
            "antall_milli": m.antall_milli,
            "kostpris_ore": m.kostpris_ore,
            "note": m.note,
            "invoice_no": m.invoice_no,
            "created_by": m.created_by,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct MovementRequest {
    produkt: String,
    dato: chrono::NaiveDate,
    /// kjop | justering (salg only ever comes from invoicing).
    kind: String,
    antall_milli: i64,
    /// Anskaffelseskost per unit (øre) — expected on kjøp.
    kostpris_ore: Option<i64>,
    note: Option<String>,
}

pub async fn register_movement(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<MovementRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::register_movement(
        &state.pool,
        company_id,
        &request.produkt,
        request.dato,
        &request.kind,
        request.antall_milli,
        request.kostpris_ore,
        request.note.as_deref(),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "registered": true })))
}

#[derive(Deserialize)]
pub struct CountLine {
    produkt: String,
    talt_milli: i64,
}

#[derive(Deserialize)]
pub struct CountRequest {
    dato: chrono::NaiveDate,
    linjer: Vec<CountLine>,
    /// Post the value adjustment as an ordinary voucher (default true).
    post: Option<bool>,
    journal: Option<String>,
    /// Defaults: 1460 (varelager), 4390 (beholdningsendring).
    lager_konto: Option<String>,
    endring_konto: Option<String>,
}

pub async fn count(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CountRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let konti = regnmed_db::TellingKonti {
        journal_code: request.journal.unwrap_or_else(|| "GL".into()),
        lager_konto: request.lager_konto.unwrap_or_else(|| "1460".into()),
        endring_konto: request.endring_konto.unwrap_or_else(|| "4390".into()),
    };
    let post = request.post.unwrap_or(true);
    let linjer: Vec<regnmed_db::TellingLinje> = request
        .linjer
        .iter()
        .map(|l| regnmed_db::TellingLinje {
            nummer: l.produkt.clone(),
            talt_milli: l.talt_milli,
        })
        .collect();
    let result = regnmed_db::varetelling(
        &state.pool,
        company_id,
        request.dato,
        &linjer,
        post.then_some(&konti),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "justeringer": result.justeringer.iter().map(|j| json!({
            "produkt": j.nummer,
            "bokfort_milli": j.bokfort_milli,
            "talt_milli": j.talt_milli,
        })).collect::<Vec<_>>(),
        "verdi_ore": result.verdi_ore,
        "bokfort_ore": result.bokfort_ore,
        "voucher": result.voucher.map(|(year, no)| format!("{year}-{no}")),
    })))
}
