//! A company's approval of regnmed in Altinn (docs/gov.md).
//!
//! - GET  /companies/{id}/altinn/system-user   where the approval stands
//! - POST /companies/{id}/altinn/system-user   request it
//! - POST /companies/{id}/altinn/system-user/check   ask Altinn, request nothing
//!
//! Approving happens in Altinn, by someone with authority at the company.

use axum::Json;
use axum::extract::{Path, State};
use regnmed_db::altinn_system_user::{self, SystemUserRow};
use regnmed_gov::altinn_system_user::CreateOutcome;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};
use crate::{AppState, SystemUserRail};

const ACCEPTED: &str = "Accepted";
const PENDING: &str = "New";
const TIMED_OUT: &str = "Timedout";
/// Ours, not Altinn's: an approval whose request Altinn no longer has.
const REVOKED: &str = "Revoked";

fn view(state: &AppState, row: Option<&SystemUserRow>) -> Value {
    let status = row.map(|r| r.status.as_str());
    json!({
        "tilgjengelig": state.system_user.is_some(),
        "status": status,
        "godkjent": status == Some(ACCEPTED),
        "godkjenningslenke": row
            .filter(|r| r.status == PENDING)
            .and_then(|r| r.confirm_url.as_deref()),
        // An approval Altinn reported without a request has nothing to look up.
        "kontrollerbar": row.is_some_and(|r| r.altinn_request_id.is_some()),
    })
}

fn refused(company_id: Uuid, e: anyhow::Error) -> ApiError {
    eprintln!("altinn system user for {company_id}: {e:#}");
    ApiError::BadRequest("Altinn svarte ikke som forventet. Prøv igjen senere.".into())
}

/// Mirrors what Altinn says about an open request or an approval.
async fn refresh(
    state: &AppState,
    rail: &SystemUserRail,
    company_id: Uuid,
    row: SystemUserRow,
) -> Result<SystemUserRow, ApiError> {
    // Revoked is rechecked too: a stray 404 from Altinn must not stick.
    let open = [PENDING, ACCEPTED, REVOKED].contains(&row.status.as_str());
    let Some(id) = row.altinn_request_id.clone().filter(|_| open) else {
        return Ok(row);
    };
    let current = rail
        .client
        .status(&rail.tokens, &id)
        .await
        .map_err(|e| refused(company_id, e))?;
    let (status, confirm_url) = match current {
        Some(current) => (
            current.status,
            current.confirm_url.or_else(|| row.confirm_url.clone()),
        ),
        None if row.status == PENDING => (TIMED_OUT.to_string(), None),
        None => (REVOKED.to_string(), None),
    };
    if status == row.status && confirm_url == row.confirm_url {
        return Ok(row);
    }
    altinn_system_user::update_status(
        &state.pool,
        company_id,
        &id,
        &status,
        confirm_url.as_deref(),
    )
    .await?;
    Ok(altinn_system_user::get(&state.pool, company_id)
        .await?
        .unwrap_or(row))
}

pub async fn status(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::CompanyAdmin).await?;
    let row = altinn_system_user::get(&state.pool, company_id).await?;
    Ok(Json(view(&state, row.as_ref())))
}

/// Never creates a request, so it is safe to run whenever the page opens.
pub async fn check(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::CompanyAdmin).await?;
    let row = altinn_system_user::get(&state.pool, company_id).await?;
    let (Some(rail), Some(row)) = (state.system_user.clone(), row.clone()) else {
        return Ok(Json(view(&state, row.as_ref())));
    };
    let row = refresh(&state, &rail, company_id, row).await?;
    Ok(Json(view(&state, Some(&row))))
}

pub async fn request(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::CompanyAdmin).await?;
    crate::terms::require_accepted(&state, company_id).await?;
    let Some(rail) = state.system_user.clone() else {
        return Err(ApiError::BadRequest(
            "Altinn-tilkobling er ikke satt opp.".into(),
        ));
    };
    let by = person
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or(&person.sub);

    let existing = altinn_system_user::get(&state.pool, company_id).await?;
    if let Some(row) = &existing
        && let Some(id) = &row.altinn_request_id
    {
        // An open request is checked, never replaced by a second one.
        if row.status == PENDING {
            let updated = refresh(&state, &rail, company_id, row.clone()).await?;
            return Ok(Json(view(&state, Some(&updated))));
        }
        if ![ACCEPTED, TIMED_OUT, REVOKED].contains(&row.status.as_str()) {
            rail.client
                .delete(&rail.tokens, id)
                .await
                .map_err(|e| refused(company_id, e))?;
        }
    }

    let settings = regnmed_db::company_settings(&state.pool, company_id).await?;
    let redirect = state
        .portal_base
        .as_deref()
        .map(|base| format!("{}/", base.trim_end_matches('/')));
    let outcome = rail
        .client
        .create(&rail.tokens, &settings.orgnumber, redirect.as_deref())
        .await
        .map_err(|e| refused(company_id, e))?;
    let row = match outcome {
        CreateOutcome::Requested(request) => SystemUserRow {
            altinn_request_id: Some(request.id),
            status: request.status,
            confirm_url: request.confirm_url,
        },
        // Any earlier request id is not this approval's, and checking it would
        // report a live approval as revoked.
        CreateOutcome::AlreadyApproved => SystemUserRow {
            altinn_request_id: None,
            status: ACCEPTED.into(),
            confirm_url: None,
        },
    };
    altinn_system_user::save(&state.pool, company_id, &row, by).await?;
    Ok(Json(view(&state, Some(&row))))
}
