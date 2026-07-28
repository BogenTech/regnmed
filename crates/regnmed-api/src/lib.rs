//! HTTP API for regnmed. Library crate so integration tests can build the
//! router; the `regnmed-api` binary is a thin wrapper (src/main.rs).

pub mod aksjebok;
pub mod anchor;
pub mod asset;
pub mod attestering;
pub mod auth;
pub mod bank;
pub mod budsjett;
pub mod currency;
pub mod dimension;
pub mod engagement;
pub mod epost;
pub mod expenses;
pub mod innboks;
pub mod integrasjon;
pub mod invoice;
pub mod invoice_template;
pub mod lonn;
pub mod mailq;
pub mod mailq_in;
pub mod marketplace;
pub mod medlemmer;
pub mod migrering;
pub mod ocr;
pub mod payments;
pub mod period;
pub mod portal;
pub mod product;
pub mod purring;
pub mod reports;
pub mod reskontro;
pub mod roller;
pub mod salgsdokument;
pub mod settings;
pub mod tilgang;
pub mod timesheet;
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
    /// Per-integration rate limiting (docs/integrations.md, #45).
    pub rate: Arc<auth::RateLimiter>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(portal::index))
        .route("/callback", get(portal::index))
        .route("/app.js", get(portal::app_js))
        .route("/theme.js", get(portal::theme_js))
        .route("/app.css", get(portal::app_css))
        .route("/manifest.webmanifest", get(portal::manifest))
        .route("/sw.js", get(portal::service_worker))
        .route("/icon-192.png", get(portal::icon_192))
        .route("/icon-512.png", get(portal::icon_512))
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
            "/companies/{company_id}/import/contacts",
            axum::routing::post(migrering::import_contacts),
        )
        .route(
            "/companies/{company_id}/import/open-items",
            axum::routing::post(migrering::import_open_items),
        )
        .route(
            "/companies/{company_id}/opening-balance",
            axum::routing::post(marketplace::opening_balance),
        )
        .route(
            "/companies/{company_id}/integrations",
            get(integrasjon::list).post(integrasjon::grant),
        )
        .route(
            "/companies/{company_id}/integrations/log",
            get(integrasjon::log),
        )
        .route(
            "/companies/{company_id}/integrations/{integration_id}/revoke",
            axum::routing::post(integrasjon::revoke),
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
            "/companies/{company_id}/mva/terminordning",
            get(reports::terminordning).post(reports::set_terminordning),
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
            "/companies/{company_id}/reports/nokkeltall",
            get(reports::nokkeltall),
        )
        .route(
            "/companies/{company_id}/reports/avvik",
            get(budsjett::avvik),
        )
        .route(
            "/companies/{company_id}/budgets",
            get(budsjett::list_budgets).post(budsjett::create_budget),
        )
        .route(
            "/companies/{company_id}/budgets/{budget_id}",
            get(budsjett::get_budget).delete(budsjett::delete_budget),
        )
        .route(
            "/companies/{company_id}/budgets/{budget_id}/lines",
            axum::routing::put(budsjett::set_lines),
        )
        .route(
            "/companies/{company_id}/budgets/{budget_id}/fastsett",
            axum::routing::post(budsjett::fastsett),
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
            "/companies/{company_id}/invoices/{invoice_id}/ehf",
            get(invoice::invoice_ehf),
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
            "/companies/{company_id}/payments/payable",
            get(payments::payable),
        )
        .route(
            "/companies/{company_id}/payments/runs",
            get(payments::list_runs).post(payments::create_run),
        )
        .route(
            "/companies/{company_id}/payments/runs/{run_id}/approve",
            axum::routing::post(payments::approve),
        )
        .route(
            "/companies/{company_id}/payments/runs/{run_id}/file",
            get(payments::file),
        )
        .route(
            "/companies/{company_id}/payments/runs/{run_id}/settle",
            axum::routing::post(payments::settle),
        )
        .route(
            "/companies/{company_id}/payments/runs/{run_id}/cancel",
            axum::routing::post(payments::cancel),
        )
        .route(
            "/companies/{company_id}/currency/rates",
            get(currency::list_rates).post(currency::add_rate),
        )
        .route(
            "/companies/{company_id}/currency/rates/fetch",
            axum::routing::post(currency::fetch_rates),
        )
        .route(
            "/companies/{company_id}/currency/regulate",
            axum::routing::post(currency::regulate),
        )
        .route("/companies/{company_id}/expenses", get(expenses::list))
        .route(
            "/companies/{company_id}/expenses/utlegg",
            axum::routing::post(expenses::create_utlegg),
        )
        .route(
            "/companies/{company_id}/expenses/kjoring",
            axum::routing::post(expenses::create_kjoring),
        )
        .route(
            "/companies/{company_id}/expenses/{expense_id}/receipt",
            get(expenses::receipt),
        )
        .route(
            "/companies/{company_id}/expenses/{expense_id}/approve",
            axum::routing::post(expenses::approve),
        )
        .route(
            "/companies/{company_id}/expenses/{expense_id}/reject",
            axum::routing::post(expenses::reject),
        )
        .route(
            "/companies/{company_id}/expenses/{expense_id}/pay",
            axum::routing::post(expenses::pay),
        )
        .route(
            "/companies/{company_id}/employees",
            get(lonn::list_employees).post(lonn::create_employee),
        )
        .route(
            "/companies/{company_id}/payroll",
            get(lonn::list_payroll).post(lonn::run_payroll),
        )
        .route(
            "/companies/{company_id}/payroll/{run_id}/slip/{employee_id}",
            get(lonn::payslip_pdf),
        )
        .route(
            "/companies/{company_id}/payroll/hours/{employee_id}",
            get(lonn::payroll_hours),
        )
        .route(
            "/companies/{company_id}/shareholders",
            get(aksjebok::list_shareholders).post(aksjebok::create_shareholder),
        )
        .route(
            "/companies/{company_id}/shareholders/transaction-types",
            get(aksjebok::transaction_types),
        )
        .route(
            "/companies/{company_id}/shareholders/{shareholder_id}/contact",
            axum::routing::put(aksjebok::update_contact),
        )
        .route(
            "/companies/{company_id}/share-events",
            get(aksjebok::list_events).post(aksjebok::create_event),
        )
        .route(
            "/companies/{company_id}/dividends",
            get(aksjebok::list_dividends).post(aksjebok::create_dividend),
        )
        .route(
            "/companies/{company_id}/reports/aksjonaeroppgave",
            get(aksjebok::oppgave),
        )
        .route(
            "/companies/{company_id}/assets",
            get(asset::list).post(asset::create),
        )
        .route(
            "/companies/{company_id}/assets/depreciate",
            axum::routing::post(asset::depreciate),
        )
        .route("/companies/{company_id}/assets/saldo", get(asset::saldo))
        .route(
            "/companies/{company_id}/assets/{asset_id}/dispose",
            axum::routing::post(asset::dispose),
        )
        .route(
            "/companies/{company_id}/assets/{asset_id}/runs",
            get(asset::runs),
        )
        .route(
            "/companies/{company_id}/products",
            get(product::list).post(product::create),
        )
        .route(
            "/companies/{company_id}/products/{nummer}",
            axum::routing::put(product::update),
        )
        .route("/companies/{company_id}/inventory", get(product::inventory))
        .route(
            "/companies/{company_id}/inventory/movements",
            get(product::movements).post(product::register_movement),
        )
        .route(
            "/companies/{company_id}/inventory/count",
            axum::routing::post(product::count),
        )
        .route(
            "/companies/{company_id}/timesheet",
            get(timesheet::list).post(timesheet::create),
        )
        .route(
            "/companies/{company_id}/timesheet/summary",
            get(timesheet::summary),
        )
        .route(
            "/companies/{company_id}/timesheet/unbilled",
            get(timesheet::unbilled),
        )
        .route(
            "/companies/{company_id}/timesheet/invoice",
            axum::routing::post(timesheet::bill),
        )
        .route(
            "/companies/{company_id}/timesheet/lock",
            get(timesheet::get_lock).put(timesheet::set_lock),
        )
        .route(
            "/companies/{company_id}/timesheet/{entry_id}",
            axum::routing::put(timesheet::update).delete(timesheet::delete),
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
            "/companies/{company_id}/quotes",
            get(salgsdokument::list_quotes).post(salgsdokument::create_quote),
        )
        .route(
            "/companies/{company_id}/quotes/{quote_id}",
            axum::routing::put(salgsdokument::update_quote),
        )
        .route(
            "/companies/{company_id}/quotes/{quote_id}/status",
            axum::routing::post(salgsdokument::quote_status),
        )
        .route(
            "/companies/{company_id}/quotes/{quote_id}/order",
            axum::routing::post(salgsdokument::quote_to_order),
        )
        .route(
            "/companies/{company_id}/quotes/{quote_id}/pdf",
            get(salgsdokument::pdf),
        )
        .route(
            "/companies/{company_id}/orders",
            get(salgsdokument::list_orders).post(salgsdokument::create_order),
        )
        .route(
            "/companies/{company_id}/orders/{order_id}/invoice",
            axum::routing::post(salgsdokument::order_to_invoice),
        )
        .route(
            "/companies/{company_id}/orders/{order_id}/pdf",
            get(salgsdokument::pdf),
        )
        .route(
            "/companies/{company_id}/invoice-templates",
            get(invoice_template::list).post(invoice_template::create),
        )
        .route(
            "/companies/{company_id}/invoice-templates/{template_id}",
            axum::routing::put(invoice_template::update),
        )
        .route(
            "/companies/{company_id}/invoice-templates/{template_id}/generate",
            axum::routing::post(invoice_template::generate),
        )
        .route(
            "/companies/{company_id}/invoice-templates/{template_id}/runs",
            get(invoice_template::runs),
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
            "/companies/{company_id}/inbox/settings",
            get(epost::settings),
        )
        .route(
            "/companies/{company_id}/inbox/settings/address",
            axum::routing::post(epost::rotate_address),
        )
        .route(
            "/companies/{company_id}/inbox/settings/senders",
            axum::routing::post(epost::add_sender),
        )
        .route(
            "/companies/{company_id}/inbox/settings/senders/{sender_id}",
            axum::routing::delete(epost::remove_sender),
        )
        .route("/companies/{company_id}/inbox/mail", get(epost::list_mail))
        .route(
            "/companies/{company_id}/inbox/mail/{mail_id}/release",
            axum::routing::post(epost::release),
        )
        .route(
            "/companies/{company_id}/inbox/mail/{mail_id}/reject",
            axum::routing::post(epost::reject),
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
            "/companies/{company_id}/inbox/{document_id}/ehf",
            get(innboks::ehf_forslag),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/forslag",
            get(innboks::forslag),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/attester",
            axum::routing::post(attestering::attester),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/attestering",
            get(attestering::trail),
        )
        .route(
            "/companies/{company_id}/attestering/policy",
            get(attestering::get_policy).post(attestering::set_policy),
        )
        .route("/companies/{company_id}/members", get(attestering::members))
        .route(
            "/companies/{company_id}/access",
            get(medlemmer::list_access),
        )
        .route(
            "/companies/{company_id}/access/history",
            get(medlemmer::access_history),
        )
        .route(
            "/companies/{company_id}/access/{person_id}",
            axum::routing::put(medlemmer::set_role).delete(medlemmer::revoke_access),
        )
        .route(
            "/companies/{company_id}/access/{person_id}/restore",
            axum::routing::post(medlemmer::restore_access),
        )
        .route(
            "/companies/{company_id}/invitations",
            get(medlemmer::list_invitations).post(medlemmer::invite),
        )
        .route(
            "/companies/{company_id}/invitations/{invitation_id}",
            axum::routing::delete(medlemmer::revoke_invitation),
        )
        .route(
            "/companies/{company_id}/roles",
            get(roller::list_roles).post(roller::create_role),
        )
        .route(
            "/companies/{company_id}/roles/history",
            get(roller::history),
        )
        .route(
            "/companies/{company_id}/roles/{role_id}",
            axum::routing::put(roller::set_rights),
        )
        .route(
            "/companies/{company_id}/roles/{role_id}/deactivate",
            axum::routing::post(roller::deactivate),
        )
        .route(
            "/companies/{company_id}/roles/{role_id}/restore",
            axum::routing::post(roller::restore),
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
    // Invitasjoner løses inn her, når vi vet hvem som spør og hvilken
    // e-postadresse tokenet bærer. En invitasjon er stilet til en
    // ADRESSE, ikke til en person — personen finnes ikke å slå opp før
    // hun har logget inn (#53, docs/auth.md).
    //
    // Samme mønster som oppdrag: tilgangen blir synlig uten ny
    // innlogging, fordi portalen kaller /me når økten starter. Å legge
    // oppslaget i AuthPerson-ekstraktoren i stedet ville kostet en
    // spørring på HVER forespørsel for noe som skjer én gang.
    let nye = regnmed_db::medlemmer::los_inn_invitasjoner(&state.pool, person.person_id).await?;

    let access = regnmed_db::company_access_for_person(&state.pool, person.person_id).await?;

    // Abonnementsstatus per selskap (#65): portalen viser banneret her,
    // og sperren selv sitter i tilgangsvakten. Ett oppslag per unikt
    // selskap — listen er kort (en persons selskaper, ikke alle).
    let mut abonnement = std::collections::HashMap::new();
    for a in &access {
        if let std::collections::hash_map::Entry::Vacant(e) = abonnement.entry(a.company_id) {
            let status = regnmed_db::abonnement::status_for(&state.pool, a.company_id).await?;
            use regnmed_core::abonnement::Status;
            let dato = match status {
                Status::Aktiv => None,
                Status::Prove { til } => Some(til),
                Status::Frist { sperres } => Some(sperres),
                Status::Sperret { siden } => Some(siden),
            };
            e.insert(json!({ "status": status.slug(), "dato": dato.map(|d| d.to_string()) }));
        }
    }

    Ok(Json(json!({
        "person_id": person.person_id,
        "nye_tilganger": nye,
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
                "abonnement": abonnement.get(&a.company_id),
            }))
            .collect::<Vec<_>>(),
    })))
}
