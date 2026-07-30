//! Abonnement i portalen (#65, #74 — docs/abonnement.md §5).
//!
//! Self-service signup goes through Stripe Checkout in SUBSCRIPTION mode:
//! the card is stored and the recurring schedule starts in one step, and
//! it keeps recurring until somebody cancels. Stripe owns the repetition
//! and the retry clock; card data never touches us.
//!
//! What stays ours, and is the reason this module is not a thin proxy:
//! the coverage rows the access guard reads, the price list the Stripe
//! Prices are created FROM, and the bookkeeping — every paid invoice
//! becomes a voucher in the ops company's hovedbok, because
//! bokføringsloven does not care who collected the money.
//!
//! The webhook is the only source of "paid", and it is idempotent all the
//! way down (unique payment_intent and stripe_invoice_id in the log).
//!
//! Without STRIPE_SECRET_KEY/STRIPE_WEBHOOK_SECRET the card rail is OFF:
//! signup writes the coverage row directly, which is how ops-agreed
//! invoicing works — the NATS pattern, no pretending.

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

/// Status, plan, price list and card — what the portal's abonnement card
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
    // The Stripe subscription, when the company is on that rail. Its
    // presence is what tells the portal to offer "si opp" rather than
    // "kontakt drift" — companies from before the Stripe rail keep the
    // old arrangement and have nothing to cancel here.
    let abo = regnmed_db::abonnement::stripe_abo_for(&state.pool, company_id).await?;
    Ok(Json(json!({
        "status": status.slug(),
        "dato": dato.map(|d| d.to_string()),
        "planer": planer,
        "kort": kort.filter(|k| k.aktiv).map(|k| json!({ "brand": k.brand, "last4": k.last4 })),
        "kort_mulig": state.stripe.is_some(),
        "abonnement": abo.map(|a| json!({
            "plan": a.plan,
            "interval": a.interval,
            "status": a.status,
            // Set once cancelled: the day coverage actually ends. Until
            // then the customer keeps what they have paid for.
            "sies_opp": a.cancel_at.map(|d| d.date_naive().to_string()),
        })),
    })))
}

#[derive(Deserialize)]
pub struct CardSetupRequest {
    /// The page the user is sent back to; defaults to the portal's front
    /// page for the company. Always relative to our own origin.
    #[serde(default)]
    pub return_path: Option<String>,
}

/// Starter kortregistreringen: oppretter (eller gjenbruker)
/// the Stripe customer and answers with the Checkout URL the portal sends
/// the user to. The card comes back via the webhook.
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
    /// `month` (default) or `year`.
    #[serde(default)]
    pub interval: Option<String>,
    #[serde(default)]
    pub return_path: Option<String>,
}

/// Finds — or creates — the Stripe Price mirroring our price list for a
/// plan and interval.
///
/// OUR list stays authoritative: the Price is created FROM it, never the
/// other way round, and a price change makes a new immutable Stripe Price
/// rather than editing one. Existing subscriptions keep the Price they
/// were created with, which is grandfathering for free — the same
/// property dated price rows give us everywhere else.
async fn sync_price(
    state: &AppState,
    cfg: &StripeCfg,
    plan: &str,
    interval: &str,
    idag: chrono::NaiveDate,
) -> Result<String, ApiError> {
    let brutto = regnmed_db::abonnement::brutto_for(&state.pool, plan, interval, idag).await?;
    if let Some((price_id, kjent)) =
        regnmed_db::abonnement::stripe_price_for(&state.pool, plan, interval).await?
    {
        if kjent == brutto {
            return Ok(price_id);
        }
    }
    let navn = format!("regnmed {plan}");
    let price_id = client(cfg)
        .create_price(&navn, brutto, interval, plan)
        .await?;
    regnmed_db::abonnement::lagre_stripe_price(
        &state.pool,
        plan,
        interval,
        brutto,
        &price_id,
        &format!("synkronisert fra prislisten {idag}"),
    )
    .await?;
    Ok(price_id)
}

/// Selvbetjent tegning. With Stripe configured this returns a Checkout
/// URL in SUBSCRIPTION mode: the card is stored and the recurring
/// schedule starts in one step, and it keeps recurring until somebody
/// cancels. Coverage is opened by the webhook, not here — the payment
/// provider confirming is what makes it real.
///
/// Without Stripe (self-hosted, no card rail) it falls back to writing
/// the coverage row directly, which is how ops-agreed invoicing works.
pub async fn start_subscription(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<StartRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::SelskapAdmin).await?;
    let idag = chrono::Utc::now().date_naive();
    let interval = request.interval.as_deref().unwrap_or("month");
    if interval != "month" && interval != "year" {
        return Err(ApiError::BadRequest(
            "intervallet må være «month» eller «year»".into(),
        ));
    }
    regnmed_db::abonnement::pris_pa(&state.pool, &request.plan, idag)
        .await
        .map_err(|_| ApiError::BadRequest("ukjent plan".into()))?;

    if regnmed_db::abonnement::stripe_abo_for(&state.pool, company_id)
        .await?
        .is_some()
    {
        return Err(ApiError::BadRequest(
            "selskapet har allerede et løpende abonnement".into(),
        ));
    }

    let Some(cfg) = &state.stripe else {
        // No card rail: ops-agreed invoicing, coverage written directly.
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
        return Ok(Json(json!({ "status": "aktiv" })));
    };

    let price_id = sync_price(&state, cfg, &request.plan, interval, idag).await?;
    let stripe = client(cfg);
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
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let path = request
        .return_path
        .unwrap_or_else(|| format!("/#/c/{company_id}/oversikt"));
    let url = format!("{origin}{path}");
    let checkout = stripe
        .create_subscription_session(&customer, &price_id, &company_id.to_string(), &url, &url)
        .await?;
    Ok(Json(json!({ "url": checkout })))
}

/// Stripe's webhook. An OPEN route (no AuthPerson) — authenticated by the
/// signature, and idempotent: the same event delivered twice changes
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
            // One Stripe account can serve several products, so sessions
            // that were never ours are delivered here too. "Not ours" has
            // to be an acknowledged no-op: a 4xx makes Stripe retry for
            // three days and then disable the endpoint over traffic we
            // were never meant to see.
            let Some(company) = object["client_reference_id"]
                .as_str()
                .and_then(|s| s.parse::<Uuid>().ok())
            else {
                return Ok(Json(json!({ "handled": "ignorert" })));
            };
            // Subscription mode carries no setup_intent — Stripe attaches
            // the card itself, and the subscription reaches us on
            // customer.subscription.created. Only the setup-mode session
            // from card_setup has a card for us to store.
            if object["mode"].as_str() == Some("subscription") {
                return Ok(Json(json!({ "handled": "abonnement_sesjon" })));
            }
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
                // without touching the books.
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
        // --- Stripe Subscriptions: the abonnement paid to US ---------
        "customer.subscription.created" | "customer.subscription.updated" => {
            let sub_id = object["id"].as_str().unwrap_or_default();
            if sub_id.is_empty() {
                return Err(ApiError::BadRequest("abonnement uten id".into()));
            }
            let status = object["status"].as_str().unwrap_or("unknown");
            let cancel_at = object["cancel_at"]
                .as_i64()
                .and_then(|t| chrono::DateTime::from_timestamp(t, 0));
            let company: Option<Uuid> = object["metadata"]["regnmed_company_id"]
                .as_str()
                .and_then(|s| s.parse().ok());
            let price = object["items"]["data"][0]["price"].clone();
            let price_id = price["id"].as_str().unwrap_or_default();
            let interval = price["recurring"]["interval"].as_str().unwrap_or("month");
            let plan = price["metadata"]["regnmed_plan"].as_str().unwrap_or("");

            if let Some(company) = company {
                let ny = regnmed_db::abonnement::lagre_stripe_abo(
                    &state.pool,
                    company,
                    sub_id,
                    price_id,
                    plan,
                    interval,
                    status,
                    "Stripe (webhook)",
                )
                .await?;
                if ny {
                    return Ok(Json(json!({ "handled": "abonnement_opprettet" })));
                }
            }
            // Already known — mirror the status. Coverage is NOT touched:
            // `past_due` at Stripe means Stripe is still retrying, and
            // locking the customer out mid-retry would take the hovedbok
            // hostage over a card that may well go through tomorrow.
            regnmed_db::abonnement::oppdater_stripe_status(&state.pool, sub_id, status, cancel_at)
                .await?;
            Ok(Json(json!({ "handled": "status_oppdatert" })))
        }
        "customer.subscription.deleted" => {
            let sub_id = object["id"].as_str().unwrap_or_default();
            let idag = chrono::Utc::now().date_naive();
            match regnmed_db::abonnement::avslutt_stripe_abo(&state.pool, sub_id, idag).await? {
                Some(_) => Ok(Json(json!({ "handled": "abonnement_avsluttet" }))),
                None => Ok(Json(json!({ "handled": "replay" }))),
            }
        }
        "invoice.paid" => {
            let stripe_invoice = object["id"].as_str().unwrap_or_default();
            let sub_id = object["subscription"].as_str().unwrap_or_default();
            if stripe_invoice.is_empty() || sub_id.is_empty() {
                // Not a subscription invoice — nothing of ours.
                return Ok(Json(json!({ "handled": "ignorert" })));
            }
            let Some(abo) =
                regnmed_db::abonnement::stripe_abo_by_subscription(&state.pool, sub_id).await?
            else {
                return Ok(Json(json!({ "handled": "ukjent_abonnement" })));
            };
            let drift = drift_company(&state).await?;
            let brutto = object["amount_paid"].as_i64().unwrap_or(0);
            let intent = object["payment_intent"].as_str().unwrap_or(stripe_invoice);
            let periode = object["lines"]["data"][0]["description"]
                .as_str()
                .unwrap_or("abonnement")
                .to_string();
            let idag = chrono::Utc::now().date_naive();
            let ny = regnmed_db::abonnement::bokfor_stripe_betaling(
                &state.pool,
                drift,
                abo.company_id,
                stripe_invoice,
                intent,
                brutto,
                &abo.plan,
                &periode,
                idag,
            )
            .await?;
            Ok(Json(
                json!({ "handled": if ny { "bokfort" } else { "replay" } }),
            ))
        }
        "invoice.payment_failed" => {
            // Logged only. Stripe's own retries chase the card, and the
            // coverage row stays open while they do — the sperre comes
            // from customer.subscription.deleted when Stripe finally
            // gives up, not from a single failed attempt.
            eprintln!(
                "Stripe-trekk feilet (abonnement {}, faktura {}) — Stripe forsøker igjen",
                object["subscription"].as_str().unwrap_or("?"),
                object["id"].as_str().unwrap_or("?")
            );
            Ok(Json(json!({ "handled": "logget" })))
        }
        _ => Ok(Json(json!({ "handled": "ignorert" }))),
    }
}

/// Selvbetjent oppsigelse. Cancels AT PERIOD END — the customer keeps
/// what they have already paid for, and the coverage row is closed by
/// `customer.subscription.deleted` when Stripe's period actually runs
/// out. Cancelling immediately would take away access that is bought and
/// paid, which is the opposite of docs/abonnement.md §1.
pub async fn cancel_subscription(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::SelskapAdmin).await?;
    let Some(cfg) = &state.stripe else {
        return Err(ApiError::BadRequest(
            "kortbetaling er ikke satt opp på denne installasjonen".into(),
        ));
    };
    let Some(abo) = regnmed_db::abonnement::stripe_abo_for(&state.pool, company_id).await? else {
        return Err(ApiError::BadRequest(
            "selskapet har ikke et Stripe-abonnement å si opp — ta kontakt med drift".into(),
        ));
    };
    let (status, cancel_at) = client(cfg)
        .cancel_subscription_at_period_end(&abo.stripe_subscription_id)
        .await?;
    let cancel_at_dt = cancel_at.and_then(|t| chrono::DateTime::from_timestamp(t, 0));
    regnmed_db::abonnement::oppdater_stripe_status(
        &state.pool,
        &abo.stripe_subscription_id,
        &status,
        cancel_at_dt,
    )
    .await?;
    Ok(Json(json!({
        "status": status,
        "gjelder_til": cancel_at_dt.map(|d| d.date_naive().to_string()),
    })))
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
