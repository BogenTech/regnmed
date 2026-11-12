//! Skattekort from Skatteetaten (docs/lonn.md «Skattekort fra
//! Skatteetaten», #46).
//!
//! - GET  /companies/{id}/tax-cards                     orders and what came back (LONN_LES)
//! - POST /companies/{id}/tax-cards?year=               order and fetch (LONN_SKRIV)
//! - POST /companies/{id}/tax-cards/{order_id}/answer   fetch an answer that was not ready
//!
//! Ordering is a call to Skatteetaten on the company's behalf, so the
//! customer terms must be accepted first, like a filing.

use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

fn rail(state: &AppState) -> Result<std::sync::Arc<crate::TaxCardRail>, ApiError> {
    state.tax_card.clone().ok_or_else(|| {
        ApiError::BadRequest(
            "henting av skattekort er ikke satt opp på denne serveren (Maskinporten)".into(),
        )
    })
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollRead).await?;
    let orders = regnmed_db::tax_card::list_orders(&state.pool, company_id).await?;
    Ok(Json(json!({
        "henting_mulig": state.tax_card.is_some(),
        "bestillinger": orders.iter().map(order_json).collect::<Vec<_>>(),
    })))
}

fn order_json(o: &regnmed_db::tax_card::OrderView) -> serde_json::Value {
    json!({
        "id": o.id,
        "inntektsaar": o.income_year,
        "referanse": o.reference,
        "antall_ansatte": o.employee_count,
        "bestilt_av": o.requested_by,
        "bestilt_tidspunkt": o.created_at,
        "besvart": !o.cards.is_empty(),
        "skattekort": o.cards.iter().map(|c| json!({
            "ansatt_id": c.employee_id,
            "navn": c.name,
            "resultat": c.result,
            "skattekort_id": c.card_id,
            "utstedt": c.issued_date,
            "satt": c.applied,
            "trekk_prosent_bp": c.applied_percent_bp,
            "trekk_tabell": c.applied_table,
            "merknad": c.note,
        })).collect::<Vec<_>>(),
    })
}

#[derive(Deserialize)]
pub struct OrderQuery {
    year: i32,
}

pub async fn order(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<OrderQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollWrite).await?;
    let rail = rail(&state)?;
    crate::terms::require_accepted(&state, company_id).await?;
    let settings = regnmed_db::company_settings(&state.pool, company_id).await?;
    let employees =
        regnmed_db::tax_card::employees_in_year(&state.pool, company_id, q.year).await?;
    if employees.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "ingen ansatte i {} å hente skattekort for",
            q.year
        )));
    }
    // shortcut: one order per call, so at most MAX_PER_ORDER (1000)
    // employees; split into several orders when a customer has more.
    if employees.len() > regnmed_gov::tax_card::MAX_PER_ORDER {
        return Err(ApiError::BadRequest(format!(
            "Skatteetaten tar høyst {} ansatte per bestilling",
            regnmed_gov::tax_card::MAX_PER_ORDER
        )));
    }
    // Skatteetaten wants someone to contact about the order: the person
    // ordering, or the company's own address for an integration.
    let Some(contact) = person.email.clone().or(settings.email) else {
        return Err(ApiError::BadRequest(
            "Skatteetaten krever en e-postadresse til den som bestiller skattekort — legg inn \
             selskapets e-post under Administrasjon → Firmaopplysninger"
                .into(),
        ));
    };
    let nins: Vec<String> = employees.into_iter().map(|(_, nin)| nin).collect();
    let order_id = Uuid::new_v4();
    let order = rail
        .client
        .order(
            &rail.tokens,
            &settings.orgnumber,
            q.year,
            &nins,
            &contact,
            order_id,
        )
        .await
        .map_err(|e| {
            ApiError::BadRequest(format!("Skatteetaten tok ikke imot bestillingen: {e:#}"))
        })?;
    regnmed_db::tax_card::record_order(
        &state.pool,
        order_id,
        company_id,
        q.year,
        &order.reference,
        &order.dialog_reference,
        nins.len(),
        person.display(),
    )
    .await?;
    // The answer was ready on the first poll in test; give it a few
    // seconds, and leave the rest to the answer endpoint.
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if fetch_answer(&state, &rail, company_id, order_id, &order.reference).await? {
            break;
        }
    }
    order_response(&state, company_id, order_id).await
}

pub async fn answer(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, order_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::PayrollWrite).await?;
    let rail = rail(&state)?;
    if let Some((reference, _)) =
        regnmed_db::tax_card::unanswered_order(&state.pool, company_id, order_id).await?
    {
        fetch_answer(&state, &rail, company_id, order_id, &reference).await?;
    }
    order_response(&state, company_id, order_id).await
}

/// Fetches and stores the answer; false while it is not ready.
async fn fetch_answer(
    state: &AppState,
    rail: &crate::TaxCardRail,
    company_id: Uuid,
    order_id: Uuid,
    reference: &str,
) -> Result<bool, ApiError> {
    let orgnumber = regnmed_db::company_settings(&state.pool, company_id)
        .await?
        .orgnumber;
    let Some(answer) = rail
        .client
        .answer(&rail.tokens, &orgnumber, reference)
        .await
        .map_err(|e| ApiError::BadRequest(format!("henting av skattekortsvaret feilet: {e:#}")))?
    else {
        return Ok(false);
    };
    let cards = regnmed_gov::tax_card::parse_answer(&answer).map_err(|status| {
        ApiError::BadRequest(format!("Skatteetaten avviste bestillingen: {status}"))
    })?;
    let received: Vec<_> = cards
        .into_iter()
        .map(|c| {
            let decision = match regnmed_gov::tax_card::decide(&c) {
                regnmed_gov::tax_card::Decision::Apply(w) => Ok(w),
                regnmed_gov::tax_card::Decision::Keep(reason) => Err(reason),
            };
            regnmed_db::tax_card::ReceivedCard {
                nin: c.nin,
                result: c.result,
                card_id: c.card_id,
                issued: c.issued,
                raw: c.raw,
                decision,
            }
        })
        .collect();
    regnmed_db::tax_card::record_answer(&state.pool, company_id, order_id, &received).await?;
    Ok(true)
}

async fn order_response(
    state: &AppState,
    company_id: Uuid,
    order_id: Uuid,
) -> Result<Json<serde_json::Value>, ApiError> {
    let orders = regnmed_db::tax_card::list_orders(&state.pool, company_id).await?;
    let order = orders
        .iter()
        .find(|o| o.id == order_id)
        .ok_or(ApiError::NotFound)?;
    Ok(Json(order_json(order)))
}
