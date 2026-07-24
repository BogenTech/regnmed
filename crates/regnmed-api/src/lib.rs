//! HTTP API for regnmed. Library crate so integration tests can build the
//! router; the `regnmed-api` binary is a thin wrapper (src/main.rs).

pub mod anchor;
pub mod auth;
pub mod bank;
pub mod dimension;
pub mod engagement;
pub mod innboks;
pub mod invoice;
pub mod mailq;
pub mod marketplace;
pub mod ocr;
pub mod period;
pub mod portal;
pub mod purring;
pub mod reports;
pub mod reskontro;
pub mod settings;
pub mod utsendelse;

use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::get};
use serde_json::json;

use auth::{ApiError, AuthPerson, Verifier};

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub verifier: Arc<Verifier>,
    /// The outbound-mail rail (regnid's JetStream stream); None when
    /// NATS_URL is not configured — send endpoints then say so.
    pub mailq: Option<async_nats::jetstream::Context>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(portal::index))
        .route("/callback", get(portal::index))
        .route("/app.js", get(portal::app_js))
        .route("/theme.js", get(portal::theme_js))
        .route("/app.css", get(portal::app_css))
        .route("/portal-config", get(portal::portal_config))
        .route("/auth/token", axum::routing::post(portal::token_exchange))
        .route("/health", get(health))
        .route("/me", get(me))
        // Public by design: root hashes only — every independent copy of
        // a root is one more witness a database rewrite cannot reach.
        .route("/anchors", get(anchor::list_anchors))
        .route(
            "/companies/{company_id}/anchors",
            get(anchor::company_anchors),
        )
        .route(
            "/companies/{company_id}/anchors/verify",
            get(anchor::verify_anchors),
        )
        .route(
            "/registry/enheter/{orgnr}",
            get(marketplace::registry_preview),
        )
        .route(
            "/companies",
            axum::routing::post(marketplace::onboard_company),
        )
        .route("/firms", axum::routing::post(marketplace::create_firm))
        .route(
            "/companies/{company_id}/import/saft",
            axum::routing::post(marketplace::import_saft),
        )
        .route(
            "/companies/{company_id}/import/saft/analyze",
            axum::routing::post(marketplace::analyze_saft),
        )
        .route(
            "/companies/{company_id}/opening-balance",
            axum::routing::post(marketplace::opening_balance),
        )
        .route("/directory/firms", get(engagement::directory))
        .route("/firms/mine", get(engagement::my_firms))
        .route("/firms/{firm_id}/requests", get(engagement::firm_requests))
        .route(
            "/firms/{firm_id}/requests/{request_id}/decision",
            axum::routing::post(engagement::decide_request),
        )
        .route("/firms/{firm_id}/clients", get(engagement::firm_clients))
        .route(
            "/companies/{company_id}/engagements",
            get(engagement::company_engagements),
        )
        .route(
            "/companies/{company_id}/engagement-requests",
            axum::routing::post(engagement::request_engagement),
        )
        .route(
            "/companies/{company_id}/engagements/{engagement_id}/end",
            axum::routing::post(engagement::end_engagement),
        )
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
            "/companies/{company_id}/reports/saldobalanse",
            get(reports::saldobalanse),
        )
        .route(
            "/companies/{company_id}/reports/kontospesifikasjon",
            get(reports::kontospesifikasjon),
        )
        .route(
            "/companies/{company_id}/reports/bokforingsspesifikasjon",
            get(reports::bokforingsspesifikasjon),
        )
        .route(
            "/companies/{company_id}/reports/resultat",
            get(reports::resultat),
        )
        .route(
            "/companies/{company_id}/reports/balanse",
            get(reports::balanse),
        )
        .route(
            "/companies/{company_id}/reports/revisjon",
            get(reports::revisjon),
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
        .route(
            "/companies/{company_id}/ocr/files",
            axum::routing::post(ocr::import_file),
        )
        .route(
            "/companies/{company_id}/ocr/payments",
            get(ocr::list_payments),
        )
        .route(
            "/companies/{company_id}/dimensions",
            get(dimension::list).post(dimension::create),
        )
        .route(
            "/companies/{company_id}/dimensions/{kind}/{code}",
            axum::routing::put(dimension::update),
        )
        .route(
            "/companies/{company_id}/parties",
            get(reskontro::list_parties).post(reskontro::create_party),
        )
        .route(
            "/companies/{company_id}/parties/{party_id}/items",
            get(reskontro::party_items),
        )
        .route(
            "/companies/{company_id}/reskontro/matches",
            axum::routing::post(reskontro::create_match),
        )
        .route(
            "/companies/{company_id}/reskontro/matches/{match_id}",
            axum::routing::delete(reskontro::delete_match),
        )
        .route(
            "/companies/{company_id}/accounts/{account_number}/reskontro",
            axum::routing::put(reskontro::set_account_reskontro),
        )
        .route(
            "/companies/{company_id}/invoices",
            get(invoice::list_invoices).post(invoice::create_invoice),
        )
        .route(
            "/companies/{company_id}/invoices/{invoice_id}/credit-note",
            axum::routing::post(invoice::credit_note),
        )
        .route(
            "/companies/{company_id}/invoices/{invoice_id}/pdf",
            get(invoice::invoice_pdf),
        )
        .route(
            "/companies/{company_id}/invoices/{invoice_id}/send",
            axum::routing::post(utsendelse::send_invoice),
        )
        .route(
            "/companies/{company_id}/invoices/{invoice_id}/reminders/{reminder_id}/send",
            axum::routing::post(utsendelse::send_reminder),
        )
        .route(
            "/companies/{company_id}/invoices/{invoice_id}/utsendelser",
            get(utsendelse::list_utsendelser),
        )
        .route(
            "/companies/{company_id}/settings",
            get(settings::get_settings).put(settings::update_settings),
        )
        .route(
            "/companies/{company_id}/parties/{party_id}/contact",
            axum::routing::put(settings::update_party_contact),
        )
        .route(
            "/companies/{company_id}/invoices/overdue",
            get(purring::overdue),
        )
        .route(
            "/companies/{company_id}/invoices/{invoice_id}/reminders",
            get(purring::list_reminders).post(purring::create_reminder),
        )
        .route(
            "/companies/{company_id}/invoices/{invoice_id}/reminders/{reminder_id}",
            get(purring::reminder_document),
        )
        .route(
            "/companies/{company_id}/inbox",
            get(innboks::list).post(innboks::upload),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/content",
            get(innboks::download),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/bokfor",
            axum::routing::post(innboks::bokfor),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/avvis",
            axum::routing::post(innboks::avvis),
        )
        .route(
            "/companies/{company_id}/period-lock",
            get(period::get_period_lock).put(period::set_period_lock),
        )
        .route(
            "/companies/{company_id}/vouchers",
            get(period::list_vouchers),
        )
        .route(
            "/companies/{company_id}/vouchers/{voucher_id}/attachments",
            get(period::list_voucher_attachments).post(period::upload_attachment),
        )
        .route(
            "/companies/{company_id}/attachments/{attachment_id}",
            get(period::download_attachment),
        )
        // Attachments and statement uploads need more than axum's 2 MB default.
        .layer(axum::extract::DefaultBodyLimit::max(20 * 1024 * 1024))
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
