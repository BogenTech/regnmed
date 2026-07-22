//! HTTP API for regnmed. Library crate so integration tests can build the
//! router; the `regnmed-api` binary is a thin wrapper (src/main.rs).

pub mod auth;
pub mod bank;
pub mod reports;

use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::get};
use serde_json::json;

use auth::{ApiError, AuthPerson, Verifier};

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub verifier: Arc<Verifier>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/me", get(me))
        .route(
            "/companies/{company_id}/reports/mva",
            get(reports::mva_report),
        )
        .route(
            "/companies/{company_id}/reports/mva-melding",
            get(reports::mva_melding),
        )
        .route(
            "/companies/{company_id}/reports/saft",
            get(reports::saft_export),
        )
        .route(
            "/companies/{company_id}/bank/statements",
            axum::routing::post(bank::import_statement),
        )
        .route(
            "/companies/{company_id}/bank/reconciliation",
            get(bank::reconciliation),
        )
        .route(
            "/companies/{company_id}/bank/matches",
            axum::routing::post(bank::create_match),
        )
        .route(
            "/companies/{company_id}/bank/matches/{bank_transaction_id}",
            axum::routing::delete(bank::delete_match),
        )
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

/// Who am I, and which companies may I act for — the resolution every
/// other endpoint will build on.
async fn me(
    State(state): State<AppState>,
    person: AuthPerson,
) -> Result<Json<serde_json::Value>, ApiError> {
    let access = regnmed_db::company_access_for_person(&state.pool, person.person_id).await?;
    Ok(Json(json!({
        "person_id": person.person_id,
        "sub": person.sub,
        "name": person.name,
        "email": person.email,
        "companies": access
            .iter()
            .map(|a| json!({
                "company_id": a.company_id,
                "orgnr": a.orgnr,
                "name": a.name,
                "access": a.access,
                "via": a.via,
            }))
            .collect::<Vec<_>>(),
    })))
}
