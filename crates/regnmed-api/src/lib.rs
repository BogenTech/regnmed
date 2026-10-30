//! HTTP API for regnmed. Library crate so integration tests can build the
//! router; the `regnmed-api` binary is a thin wrapper (src/main.rs).

pub mod accrual;
pub mod agent_task;
pub mod aksjebok;
pub mod anchor;
pub mod approval;
pub mod asset;
pub mod auth;
pub mod balance_documentation;
pub mod bank;
pub mod budget;
pub mod cash_register;
pub mod chart_of_accounts;
pub mod currency;
pub mod dimension;
pub mod dispatch;
pub mod email;
pub mod engagement;
pub mod expenses;
pub mod firm_members;
pub mod herding;
pub mod inbox;
pub mod integration;
pub mod invoice;
pub mod invoice_template;
pub mod lonn;
pub mod mailq;
pub mod mailq_in;
pub mod marketplace;
pub mod members;
pub mod migration;
pub mod ocr;
pub mod payments;
pub mod period;
pub mod platform;
pub mod portal;
pub mod product;
pub mod purring;
pub mod reports;
pub mod reskontro;
pub mod roles;
pub mod salgsdokument;
pub mod settings;
pub mod subscription;
pub mod tilgang;
pub mod timesheet;
pub mod year_end;

use std::sync::Arc;

use axum::response::{IntoResponse, Response};
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
    /// The card rail (#74, docs/abonnement.md §5); None when
    /// STRIPE_SECRET_KEY/STRIPE_WEBHOOK_SECRET are unset — the endpoints
    /// then say so instead of pretending.
    pub stripe: Option<StripeCfg>,
    /// Driftsselskapets orgnr (REGNMED_DRIFT_ORGNR) — hovedboken
    /// abonnementstrekkene bokføres i.
    pub drift_orgnr: Option<String>,
    /// The portal's public URL (PORTAL_BASE_URL), used for the link in
    /// the invitation mail (#66). Deliberately configuration and never
    /// the request's Host header: a mail we send must not be addressable
    /// by whoever made the request.
    pub portal_base: Option<String>,
    /// Seals the customer's own LLM key at rest (AGENT_SEALING_KEY, 64 hex
    /// characters — docs/agent.md §8). None when it is unset, and then
    /// the hosted-agent endpoints say so rather than storing a key we
    /// cannot protect. It is configuration on the process, never a row:
    /// the database holds ciphertext and nothing in it can open that.
    pub sealing_key: Option<regnmed_core::secret::SealingKey>,
    /// Lets the portal create an agent's OIDC identity instead of
    /// asking the customer to run a CLI command (docs/agent.md §8.6).
    /// None when it is unconfigured, and then the agent form says a
    /// client_id is needed — the old path still works, it just is not
    /// the only one.
    pub regnid: Option<RegnidCfg>,
}

/// regnmed's own credentials at the IdP, for provisioning agent
/// identities. Both or neither: half of this is a mistake, not a mode.
#[derive(Clone)]
pub struct RegnidCfg {
    pub client_id: String,
    pub client_secret: String,
    /// Overridable so tests can point at a mock; defaults to the
    /// configured OIDC issuer, which is the same service.
    pub base_url: Option<String>,
}

#[derive(Clone)]
pub struct StripeCfg {
    pub secret_key: String,
    pub webhook_secret: String,
    /// Overridable base URL so tests can point at a mock.
    pub api_base: Option<String>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(portal::index))
        .route("/callback", get(portal::index))
        .route("/assets/{*path}", get(portal::asset))
        .route("/manifest.webmanifest", get(portal::manifest))
        .route("/sw.js", get(portal::service_worker))
        .route("/icon-192.png", get(portal::icon_192))
        .route("/icon-512.png", get(portal::icon_512))
        // /ny var portalens adresse mens migreringen pågikk (#76).
        .route("/ny", get(portal::ny_redirect))
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
            "/companies/{company_id}/import/saft/log",
            axum::routing::get(marketplace::saft_import_log),
        )
        .route(
            "/companies/{company_id}/import/contacts",
            axum::routing::post(migration::import_contacts),
        )
        .route(
            "/companies/{company_id}/import/open-items",
            axum::routing::post(migration::import_open_items),
        )
        .route(
            "/companies/{company_id}/opening-balance",
            axum::routing::post(marketplace::opening_balance),
        )
        .route(
            "/companies/{company_id}/integrations",
            get(integration::list).post(integration::grant),
        )
        .route(
            "/companies/{company_id}/integrations/log",
            get(integration::log),
        )
        // One call creates the whole agent — identity included
        // (docs/agent.md §8.6). `POST /integrations` above still takes
        // an existing client_id, for an agent the customer runs
        // themselves.
        .route(
            "/companies/{company_id}/agents",
            axum::routing::post(integration::opprett_agent),
        )
        // Oppgaver til en agent (#103): the owner says what to do, the
        // agent reports back, and the trail outlives the chat it was
        // asked in.
        .route(
            "/companies/{company_id}/agent/oppgaver",
            get(agent_task::list).post(agent_task::opprett),
        )
        .route(
            "/companies/{company_id}/agent/oppgaver/{oppgave_id}",
            get(agent_task::hent),
        )
        .route(
            "/companies/{company_id}/agent/oppgaver/{oppgave_id}/svar",
            axum::routing::post(agent_task::svar),
        )
        .route(
            "/companies/{company_id}/integrations/{integration_id}",
            axum::routing::put(integration::endre),
        )
        // Vertsagent: kundens egen LLM-leverandør (docs/agent.md §8).
        // Nøkkelen går inn forseglet og har ingen vei ut herfra.
        .route(
            "/companies/{company_id}/integrations/{integration_id}/llm",
            get(integration::llm)
                .put(integration::sett_llm)
                .delete(integration::fjern_llm),
        )
        .route(
            "/companies/{company_id}/integrations/{integration_id}/rolle",
            axum::routing::put(integration::rolle),
        )
        .route(
            "/companies/{company_id}/integrations/{integration_id}/revoke",
            axum::routing::post(integration::revoke),
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
            "/firms/{firm_id}/platform-access",
            get(platform::firm_platform_access),
        )
        .route("/firms/{firm_id}/access", get(firm_members::list_access))
        .route(
            "/firms/{firm_id}/access/history",
            get(firm_members::access_history),
        )
        .route(
            "/firms/{firm_id}/access/{person_id}",
            axum::routing::put(firm_members::set_role).delete(firm_members::revoke_access),
        )
        .route(
            "/firms/{firm_id}/access/{person_id}/restore",
            axum::routing::post(firm_members::restore_access),
        )
        .route(
            "/firms/{firm_id}/invitations",
            get(firm_members::list_invitations).post(firm_members::invite),
        )
        .route(
            "/firms/{firm_id}/invitations/{invitasjon_id}",
            axum::routing::delete(firm_members::revoke_invitation),
        )
        .route(
            "/firms/{firm_id}/invitations/{invitasjon_id}/resend",
            axum::routing::post(firm_members::resend_invitation),
        )
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
            get(reports::vat_report),
        )
        .route(
            "/companies/{company_id}/mva/terminordning",
            get(reports::period_scheme).post(reports::set_period_scheme),
        )
        .route(
            "/companies/{company_id}/reports/mva-melding",
            get(reports::vat_return),
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
            get(reports::account_specification),
        )
        .route(
            "/companies/{company_id}/reports/bokforingsspesifikasjon",
            get(reports::bokforingsspesifikasjon),
        )
        .route(
            "/companies/{company_id}/reports/kundespesifikasjon",
            get(reports::kundespesifikasjon),
        )
        .route(
            "/companies/{company_id}/reports/leverandorspesifikasjon",
            get(reports::leverandorspesifikasjon),
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
            "/companies/{company_id}/reports/prosjekt",
            get(reports::prosjektlonnsomhet),
        )
        .route(
            "/companies/{company_id}/reports/nokkeltall",
            get(reports::nokkeltall),
        )
        .route("/companies/{company_id}/reports/avvik", get(budget::avvik))
        .route(
            "/companies/{company_id}/budgets",
            get(budget::list_budgets).post(budget::create_budget),
        )
        .route(
            "/companies/{company_id}/budgets/{budget_id}",
            get(budget::get_budget).delete(budget::delete_budget),
        )
        .route(
            "/companies/{company_id}/budgets/{budget_id}/lines",
            axum::routing::put(budget::set_lines),
        )
        .route(
            "/companies/{company_id}/budgets/{budget_id}/fastsett",
            axum::routing::post(budget::fastsett),
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
            "/companies/{company_id}/dimensions/prosjekt/{code}/satser",
            get(dimension::list_satser).post(dimension::set_sats),
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
            "/companies/{company_id}/accounts",
            get(chart_of_accounts::list_accounts).post(chart_of_accounts::create_account),
        )
        .route(
            "/companies/{company_id}/accounts/{account_number}",
            axum::routing::put(chart_of_accounts::update_account),
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
            axum::routing::post(dispatch::send_invoice),
        )
        .route(
            "/companies/{company_id}/invoices/{invoice_id}/reminders/{reminder_id}/send",
            axum::routing::post(dispatch::send_reminder),
        )
        .route(
            "/companies/{company_id}/invoices/{invoice_id}/utsendelser",
            get(dispatch::list_dispatches),
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
            "/companies/{company_id}/employees/{employee_id}/link",
            axum::routing::post(lonn::link_employee).delete(lonn::unlink_employee),
        )
        .route(
            "/companies/{company_id}/employees/{employee_id}/link/history",
            get(lonn::employee_link_history),
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
            "/companies/{company_id}/arsavslutning",
            get(year_end::list).post(year_end::avslutt),
        )
        .route(
            "/companies/{company_id}/arsavslutning/{ar}/forslag",
            get(year_end::forslag),
        )
        .route(
            "/companies/{company_id}/kassaoppgjor",
            axum::routing::post(cash_register::dagsoppgjor),
        )
        .route(
            "/companies/{company_id}/kassaoppgjor/rapport",
            axum::routing::post(cash_register::dagsoppgjor_med_rapport),
        )
        .route(
            "/companies/{company_id}/balansedokumentasjon",
            get(balance_documentation::status).post(balance_documentation::avstem),
        )
        .route(
            "/companies/{company_id}/balansedokumentasjon/vedlegg",
            axum::routing::post(balance_documentation::avstem_med_vedlegg),
        )
        .route(
            "/companies/{company_id}/balansedokumentasjon/historikk",
            get(balance_documentation::historikk),
        )
        .route(
            "/companies/{company_id}/balansedokumentasjon/{id}/vedlegg",
            get(balance_documentation::vedlegg),
        )
        .route(
            "/companies/{company_id}/periodiseringer",
            get(accrual::list).post(accrual::create),
        )
        .route(
            "/companies/{company_id}/periodiseringer/{id}/stopp",
            axum::routing::post(accrual::stopp),
        )
        .route(
            "/companies/{company_id}/periodiseringer/{id}/kjor",
            axum::routing::post(accrual::kjor),
        )
        .route(
            "/companies/{company_id}/periodiseringer/{id}/kjoringer",
            get(accrual::runs),
        )
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
            "/companies/{company_id}/settings/registrering",
            axum::routing::post(settings::set_registrering),
        )
        .route(
            "/companies/{company_id}/subscription",
            get(subscription::subscription_status).post(subscription::start_subscription),
        )
        .route(
            "/companies/{company_id}/subscription/card-setup",
            axum::routing::post(subscription::card_setup),
        )
        .route(
            "/companies/{company_id}/subscription/cancel",
            axum::routing::post(subscription::cancel_subscription),
        )
        // Open route: authenticated by the webhook signature, not by a token.
        // Namespaced by provider because the card rail is deliberately
        // replaceable (docs/abonnement.md §5) — a later Nets Easy or Dintero
        // endpoint sits beside this one instead of renaming it.
        .route(
            "/webhooks/stripe",
            axum::routing::post(subscription::stripe_webhook),
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
            get(inbox::list).post(inbox::upload),
        )
        .route(
            "/companies/{company_id}/inbox/settings",
            get(email::settings),
        )
        .route(
            "/companies/{company_id}/inbox/settings/address",
            axum::routing::post(email::rotate_address),
        )
        .route(
            "/companies/{company_id}/inbox/settings/senders",
            axum::routing::post(email::add_sender),
        )
        .route(
            "/companies/{company_id}/inbox/settings/senders/{sender_id}",
            axum::routing::delete(email::remove_sender),
        )
        .route("/companies/{company_id}/inbox/mail", get(email::list_mail))
        .route(
            "/companies/{company_id}/inbox/mail/{mail_id}/release",
            axum::routing::post(email::release),
        )
        .route(
            "/companies/{company_id}/inbox/mail/{mail_id}/reject",
            axum::routing::post(email::reject),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/content",
            get(inbox::download),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/bokfor",
            axum::routing::post(inbox::bokfor),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/avvis",
            axum::routing::post(inbox::avvis),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/ehf",
            get(inbox::ehf_forslag),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/forslag",
            get(inbox::forslag),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/attester",
            axum::routing::post(approval::attester),
        )
        .route(
            "/companies/{company_id}/inbox/{document_id}/attestering",
            get(approval::trail),
        )
        .route(
            "/companies/{company_id}/attestering/policy",
            get(approval::get_policy).post(approval::set_policy),
        )
        .route("/companies/{company_id}/members", get(approval::members))
        .route("/companies/{company_id}/access", get(members::list_access))
        .route(
            "/companies/{company_id}/access/history",
            get(members::access_history),
        )
        .route(
            "/companies/{company_id}/access/{person_id}",
            axum::routing::put(members::set_role).delete(members::revoke_access),
        )
        .route(
            "/companies/{company_id}/access/{person_id}/restore",
            axum::routing::post(members::restore_access),
        )
        .route(
            "/companies/{company_id}/invitations",
            get(members::list_invitations).post(members::invite),
        )
        .route(
            "/companies/{company_id}/invitations/{invitation_id}",
            axum::routing::delete(members::revoke_invitation),
        )
        .route(
            "/companies/{company_id}/invitations/{invitation_id}/resend",
            axum::routing::post(dispatch::resend_invitation),
        )
        .route(
            "/companies/{company_id}/roles",
            get(roles::list_roles).post(roles::create_role),
        )
        .route("/companies/{company_id}/roles/history", get(roles::history))
        .route(
            "/companies/{company_id}/roles/{role_id}",
            axum::routing::put(roles::set_rights),
        )
        .route(
            "/companies/{company_id}/roles/{role_id}/deactivate",
            axum::routing::post(roles::deactivate),
        )
        .route(
            "/companies/{company_id}/roles/{role_id}/restore",
            axum::routing::post(roles::restore),
        )
        .route(
            "/companies/{company_id}/period-lock",
            get(period::get_period_lock).put(period::set_period_lock),
        )
        .route(
            "/companies/{company_id}/vouchers",
            get(period::list_vouchers).post(chart_of_accounts::post_manual_voucher),
        )
        .route(
            "/companies/{company_id}/vouchers/{voucher_id}/attachments",
            get(period::list_voucher_attachments).post(period::upload_attachment),
        )
        .route(
            "/companies/{company_id}/attachments/{attachment_id}",
            get(period::download_attachment),
        )
        // Vendor access made visible to the one it touched (docs/auth.md §8):
        // the company's own admins read what the platform did here.
        .route(
            "/companies/{company_id}/platform-access",
            get(platform::company_platform_access),
        )
        // The /platform sub-router (docs/auth.md §8) — its own guard + log
        // middleware; nothing here reaches any company's ledger.
        .merge(platform::plattform_router(state.clone()))
        // Attachments and statement uploads need more than axum's 2 MB default.
        .layer(axum::extract::DefaultBodyLimit::max(20 * 1024 * 1024))
        // Hardening on EVERY response (#64): nosniff and Referrer-Policy
        // always, CSP on HTML. One seam, so no endpoint can forget.
        .layer(axum::middleware::from_fn(herding::security_headers))
        .layer(catch_panic_layer())
        .with_state(state)
}

/// One bad request must NEVER take the process down for everybody else.
///
/// Two things ensure that, and they are different: the process survives
/// because we build with `panic=unwind` and tokio isolates the panicking
/// task (which is why `panic="abort"` is left out of the release profile
/// — see Cargo.toml), while the CLIENT gets a response because this layer
/// exists. Without the layer the connection would simply be cut, with no
/// status.
///
/// The layer is built here, in one place, so the test can exercise
/// exactly the layer `router()` mounts.
pub fn catch_panic_layer()
-> tower_http::catch_panic::CatchPanicLayer<fn(Box<dyn std::any::Any + Send + 'static>) -> Response>
{
    tower_http::catch_panic::CatchPanicLayer::custom(
        panikk_til_500 as fn(Box<dyn std::any::Any + Send + 'static>) -> Response,
    )
}

/// Turns a panic into a 500. The detail is logged server-side; the client gets
/// en nøytral melding — en panikk sier ofte noe om interne data.
fn panikk_til_500(panikk: Box<dyn std::any::Any + Send + 'static>) -> Response {
    let detalj = panikk
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panikk.downcast_ref::<&str>().copied())
        .unwrap_or("(ukjent panikk)");
    eprintln!("panikk i forespørselshåndtering: {detalj}");
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        r#"{"error":"intern feil"}"#,
    )
        .into_response()
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
    // Invitations are redeemed here, once we know who is asking and
    // which e-mail address the token carries. An invitation is addressed
    // to an ADDRESS, not to a person — the person does not exist to look
    // up before they have logged in (#53, docs/auth.md).
    //
    // The same pattern as oppdrag: the access becomes visible without a
    // fresh login, because the portal calls /me when the session starts.
    // Putting the lookup in the extractor would cost a query on EVERY
    // request for something that happens once.
    let nye = regnmed_db::members::los_inn_invitasjoner(&state.pool, person.person_id).await?
        + regnmed_db::firm_members::los_inn_invitasjoner(&state.pool, person.person_id).await?;

    let access = regnmed_db::company_access_for_person(&state.pool, person.person_id).await?;

    // Platform role (docs/auth.md §8), so the portal can show the
    // Plattform view. Deliberately separate from `companies`: a platform
    // role grants no company access and never appears in that list.
    let plattform = if person.integration.is_none() {
        regnmed_db::active_platform_role(&state.pool, person.person_id).await?
    } else {
        None
    };

    // Abonnementsstatus per selskap (#65): portalen viser banneret her,
    // and the block itself sits in the access guard. One lookup per
    // distinct company — the list is short (a person's companies, not all).
    let mut subscription = std::collections::HashMap::new();
    for a in &access {
        if let std::collections::hash_map::Entry::Vacant(e) = subscription.entry(a.company_id) {
            let status = regnmed_db::subscription::status_for(&state.pool, a.company_id).await?;
            use regnmed_core::subscription::Status;
            let date = match status {
                Status::Aktiv => None,
                Status::Trial { til } => Some(til),
                Status::Grace { sperres } => Some(sperres),
                Status::Blocked { siden } => Some(siden),
            };
            e.insert(json!({ "status": status.slug(), "dato": date.map(|d| d.to_string()) }));
        }
    }

    // Rettighetene per selskap, so the portal can show only controls
    // that will succeed. DISPLAY ONLY — the server's guard decides, the
    // portal just stops offering buttons that would 403 (docs/auth.md).
    let mut rettigheter = std::collections::HashMap::new();
    for a in &access {
        if let std::collections::hash_map::Entry::Vacant(e) = rettigheter.entry(a.company_id) {
            let slugs = tilgang::slaa_opp(&state, person.person_id, a.company_id)
                .await?
                .map(|t| t.rettighet_slugs())
                .unwrap_or_default();
            e.insert(slugs);
        }
    }

    Ok(Json(json!({
        "person_id": person.person_id,
        "nye_tilganger": nye,
        "sub": person.sub,
        "name": person.name,
        "email": person.email,
        "plattform": plattform.map(|p| json!({
            "rolle": p.rolle,
            "valid_to": p.valid_to.to_string(),
        })),
        "companies": access
            .iter()
            .map(|a| json!({
                "company_id": a.company_id,
                "orgnr": a.orgnr,
                "name": a.name,
                "access": a.access,
                "via": a.via,
                "abonnement": subscription.get(&a.company_id),
                "rettigheter": rettigheter.get(&a.company_id),
            }))
            .collect::<Vec<_>>(),
    })))
}
