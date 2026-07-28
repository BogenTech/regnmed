//! Abonnement i portalen + kortskinnen (#65, #74 — docs/abonnement.md).
//!
//! Selvbetjeningen er kort-først: admin legger inn kort (Stripe
//! Checkout i setup-modus — kortdata berører oss aldri) og starter
//! abonnementet selv. Webhooken er eneste kilde til «betalt», og den er
//! idempotent hele veien ned (unik payment_intent i loggen).
//!
//! Uten STRIPE_SECRET_KEY/STRIPE_WEBHOOK_SECRET er kortskinnen AV og
//! endepunktene sier det — NATS-mønsteret, ingen late-som.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};
use crate::{AppState, StripeCfg};

fn client(cfg: &StripeCfg) -> regnmed_gov::stripe::Stripe {
    regnmed_gov::stripe::Stripe::new(&cfg.secret_key, cfg.api_base.as_deref())
}

/// Status, plan, prisliste og kort — det portalens abonnementskort
/// trenger i ett kall.
pub async fn subscription_status(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::SelskapLes).await?;
    let status = regnmed_db::abonnement::status_for(&state.pool, company_id).await?;
    use regnmed_core::abonnement::Status;
    let dato = match status {
        Status::Aktiv => None,
        Status::Prove { til } => Some(til),
        Status::Frist { sperres } => Some(sperres),
        Status::Sperret { siden } => Some(siden),
    };
    let kort = regnmed_db::abonnement::kort_for(&state.pool, company_id).await?;
    let priser = regnmed_db::abonnement::list_priser(&state.pool).await?;
    // Nyeste gjeldende pris per plan.
    let idag = chrono::Utc::now().date_naive();
    let mut sett: Vec<&str> = Vec::new();
    let planer: Vec<serde_json::Value> = priser
        .iter()
        .filter(|p| p.valid_from <= idag)
        .filter(|p| {
            if sett.contains(&p.plan.as_str()) {
                false
            } else {
                sett.push(&p.plan);
                true
            }
        })
        .map(|p| json!({ "plan": p.plan, "pris_ore_per_mnd": p.pris_ore_per_mnd }))
        .collect();
    Ok(Json(json!({
        "status": status.slug(),
        "dato": dato.map(|d| d.to_string()),
        "planer": planer,
        "kort": kort.filter(|k| k.aktiv).map(|k| json!({ "brand": k.brand, "last4": k.last4 })),
        "kort_mulig": state.stripe.is_some(),
    })))
}

#[derive(Deserialize)]
pub struct CardSetupRequest {
    /// Siden brukeren sendes tilbake til; default portalens forside for
    /// selskapet. Alltid relativ til vår egen origin.
    #[serde(default)]
    pub return_path: Option<String>,
}

/// Starter kortregistreringen: oppretter (eller gjenbruker)
/// Stripe-kunden og svarer med Checkout-URL-en portalen sender
/// brukeren til. Kortet kommer tilbake via webhooken.
pub async fn card_setup(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<CardSetupRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::SelskapAdmin).await?;
    let Some(cfg) = &state.stripe else {
        return Err(ApiError::BadRequest(
            "kortbetaling er ikke satt opp på denne installasjonen (STRIPE_SECRET_KEY mangler)"
                .into(),
        ));
    };
    let stripe = client(cfg);

    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let path = request
        .return_path
        .unwrap_or_else(|| format!("/#/c/{company_id}/oversikt"));
    let success = format!("{origin}{path}");

    let customer = match regnmed_db::abonnement::kort_for(&state.pool, company_id).await? {
        Some(kort) => kort.stripe_customer_id,
        None => {
            let (navn, orgnr): (String, String) =
                sqlx::query_as("select name, orgnr from company where id = $1")
                    .bind(company_id)
                    .fetch_one(&state.pool)
                    .await
                    .map_err(anyhow::Error::from)?;
            stripe
                .create_customer(&company_id.to_string(), &navn, &orgnr)
                .await?
        }
    };
    let url = stripe
        .create_setup_session(&customer, &company_id.to_string(), &success, &success)
        .await?;
    Ok(Json(json!({ "url": url })))
}

#[derive(Deserialize)]
pub struct StartRequest {
    pub plan: String,
}

/// Selvbetjent tegning: admin starter abonnementet fra portalen.
/// Kort-først — uten aktivt kort må fakturaavtale gjøres med drift.
pub async fn start_subscription(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<StartRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::SelskapAdmin).await?;
    let idag = chrono::Utc::now().date_naive();
    regnmed_db::abonnement::pris_pa(&state.pool, &request.plan, idag)
        .await
        .map_err(|_| ApiError::BadRequest("ukjent plan".into()))?;
    if state.stripe.is_some() {
        let kort = regnmed_db::abonnement::kort_for(&state.pool, company_id).await?;
        if !kort.map(|k| k.aktiv).unwrap_or(false) {
            return Err(ApiError::BadRequest(
                "legg inn et betalingskort først — kortet er standardveien; fakturaavtale gjøres med drift".into(),
            ));
        }
    }
    if regnmed_db::abonnement::status_for(&state.pool, company_id)
        .await?
        .slug()
        == "aktiv"
    {
        return Err(ApiError::BadRequest(
            "abonnementet er allerede aktivt".into(),
        ));
    }
    let navn = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::abonnement::tegn(
        &state.pool,
        company_id,
        &request.plan,
        idag,
        None,
        &format!("selvbetjent i portalen av {navn}"),
        navn,
    )
    .await?;
    Ok(Json(json!({ "status": "aktiv" })))
}

/// Stripes webhook. ÅPEN rute (ingen AuthPerson) — autentisert av
/// signaturen, og idempotent: samme hendelse levert to ganger endrer
/// ingenting andre gangen.
pub async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(cfg) = &state.stripe else {
        return Err(ApiError::BadRequest(
            "kortskinnen er ikke konfigurert".into(),
        ));
    };
    let signature = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let now = chrono::Utc::now().timestamp();
    if regnmed_gov::stripe::verify_webhook(&body, signature, &cfg.webhook_secret, 300, now).is_err()
    {
        return Err(ApiError::BadRequest("ugyldig webhook-signatur".into()));
    }
    let event: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| ApiError::BadRequest("ugyldig JSON".into()))?;
    let object = &event["data"]["object"];

    match event["type"].as_str().unwrap_or("") {
        "checkout.session.completed" => {
            let company: Uuid = object["client_reference_id"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| ApiError::BadRequest("sesjon uten selskapsreferanse".into()))?;
            let customer = object["customer"].as_str().unwrap_or_default().to_string();
            let setup_intent = object["setup_intent"].as_str().unwrap_or_default();
            if customer.is_empty() || setup_intent.is_empty() {
                return Err(ApiError::BadRequest(
                    "sesjon uten kunde/setup_intent".into(),
                ));
            }
            let stripe = client(cfg);
            let pm = stripe.setup_intent_payment_method(setup_intent).await?;
            let (brand, last4) = stripe.payment_method_card(&pm).await.unwrap_or_default();
            regnmed_db::abonnement::lagre_kort(
                &state.pool,
                company,
                &customer,
                &pm,
                &brand,
                &last4,
            )
            .await?;
            Ok(Json(json!({ "handled": "kort_lagret" })))
        }
        kind @ ("payment_intent.succeeded" | "payment_intent.payment_failed") => {
            let Some(invoice_id) = object["metadata"]["invoice_id"]
                .as_str()
                .and_then(|s| s.parse::<Uuid>().ok())
            else {
                // Ikke vårt trekk (manuelt i dashboardet e.l.) — kvitter
                // uten å røre bøkene.
                return Ok(Json(json!({ "handled": "ignorert" })));
            };
            let drift = drift_company(&state).await?;
            let intent_id = object["id"].as_str().unwrap_or_default();
            let succeeded = kind == "payment_intent.succeeded";
            let belop = if succeeded {
                object["amount_received"].as_i64().unwrap_or(0)
            } else {
                object["amount"].as_i64().unwrap_or(0)
            };
            let detail = object["last_payment_error"]["message"].as_str();
            let ny = regnmed_db::abonnement::registrer_kortbetaling(
                &state.pool,
                drift,
                invoice_id,
                intent_id,
                succeeded,
                belop,
                detail,
            )
            .await?;
            Ok(Json(
                json!({ "handled": if ny { "bokfort" } else { "replay" } }),
            ))
        }
        _ => Ok(Json(json!({ "handled": "ignorert" }))),
    }
}

async fn drift_company(state: &AppState) -> Result<Uuid, ApiError> {
    let Some(orgnr) = &state.drift_orgnr else {
        return Err(ApiError::BadRequest(
            "REGNMED_DRIFT_ORGNR er ikke satt — webhooken vet ikke hvilken hovedbok trekket hører hjemme i".into(),
        ));
    };
    let id: Option<Uuid> = sqlx::query_scalar("select id from company where orgnr = $1")
        .bind(orgnr)
        .fetch_optional(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    id.ok_or_else(|| ApiError::BadRequest("driftsselskapet finnes ikke i databasen".into()))
}
