//! A company's approval of regnmed in Altinn (docs/gov.md).
//!
//! - GET  /companies/{id}/altinn/system-user   where the approval stands
//! - POST /companies/{id}/altinn/system-user   request it, or check it again
//!
//! Approving happens in Altinn, by someone with authority at the company.

use axum::Json;
use axum::extract::{Path, State};
use regnmed_db::altinn_system_user::{self, SystemUserRow};
use regnmed_gov::altinn_system_user::CreateOutcome;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

const ACCEPTED: &str = "Accepted";
const PENDING: &str = "New";
const TIMED_OUT: &str = "Timedout";

fn view(state: &AppState, row: Option<&SystemUserRow>) -> Value {
    let status = row.map(|r| r.status.as_str());
    json!({
        "tilgjengelig": state.system_user.is_some(),
        "status": status,
        "godkjent": status == Some(ACCEPTED),
        "godkjenningslenke": row
            .filter(|r| r.status == PENDING)
            .and_then(|r| r.confirm_url.as_deref()),
    })
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
    let refused = |e: anyhow::Error| {
        eprintln!("altinn system user for {company_id}: {e:#}");
        ApiError::BadRequest("Altinn svarte ikke som forventet. Prøv igjen senere.".into())
    };

    let existing = altinn_system_user::get(&state.pool, company_id).await?;
    if let Some(row) = &existing
        && let Some(id) = &row.altinn_request_id
    {
        // An open request is checked, never replaced by a second one.
        if row.status == PENDING {
            let updated = match rail
                .client
                .status(&rail.tokens, id)
                .await
                .map_err(refused)?
            {
                Some(current) => SystemUserRow {
                    altinn_request_id: Some(current.id),
                    status: current.status,
                    confirm_url: current.confirm_url.or_else(|| row.confirm_url.clone()),
                },
                None => SystemUserRow {
                    altinn_request_id: Some(id.clone()),
                    status: TIMED_OUT.into(),
                    confirm_url: None,
                },
            };
            altinn_system_user::save(&state.pool, company_id, &updated, by).await?;
            return Ok(Json(view(&state, Some(&updated))));
        }
        if row.status != ACCEPTED && row.status != TIMED_OUT {
            rail.client
                .delete(&rail.tokens, id)
                .await
                .map_err(refused)?;
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
        .map_err(refused)?;
    let row = match outcome {
        CreateOutcome::Requested(request) => SystemUserRow {
            altinn_request_id: Some(request.id),
            status: request.status,
            confirm_url: request.confirm_url,
        },
        CreateOutcome::AlreadyApproved => SystemUserRow {
            altinn_request_id: existing.and_then(|r| r.altinn_request_id),
            status: ACCEPTED.into(),
            confirm_url: None,
        },
    };
    altinn_system_user::save(&state.pool, company_id, &row, by).await?;
    Ok(Json(view(&state, Some(&row))))
}
