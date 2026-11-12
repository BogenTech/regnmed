use std::sync::Arc;

use anyhow::{Context, Result};
use regnmed_api::{AppState, auth::Verifier, router};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL is not set")?;
    let pool = regnmed_db::connect(&database_url)
        .await
        .context("connecting to database")?;

    // Fails here on a half-configured object store, not on the first upload.
    println!("{}", regnmed_db::blob::blobs().describe());

    let verifier = Arc::new(Verifier::from_env().await?);
    let mailq = regnmed_api::mailq::connect_from_env()
        .await
        .context("connecting to the mail queue")?;
    match &mailq {
        Some(_) => println!("mail rail connected (NATS)"),
        None => println!("mail rail not configured (NATS_URL unset) — utsendelse disabled"),
    }
    // E-post-inn (docs/epost-inn.md, #35) rides the same rail: with a
    // mail queue configured we consume received mail into the innboks
    // in the background; without one, reception simply does not exist.
    // The agent work queue rides the same connection (docs/agent.md
    // §8.7). Ensured here so a misconfigured stream fails at startup
    // rather than at the first task somebody creates.
    if let Some(js) = mailq.as_ref() {
        regnmed_mail::taskqueue::ensure_stream(js).await?;
        println!("agent queue ready ({})", regnmed_mail::taskqueue::SUBJECT);
    }
    if let Some(js) = mailq.clone() {
        let pool = pool.clone();
        tokio::spawn(async move {
            if let Err(e) = regnmed_api::mailq_in::run(js, pool).await {
                eprintln!("e-post-inn stoppet: {e:#}");
            }
        });
        println!("e-post-inn lytter på {}", regnmed_api::mailq_in::SUBJECT);
    }

    // Kortskinnen (#74): begge nøklene eller ingen — en halv
    // configuration is an error, not a state.
    let stripe = match (
        std::env::var("STRIPE_SECRET_KEY").ok(),
        std::env::var("STRIPE_WEBHOOK_SECRET").ok(),
    ) {
        (Some(secret_key), Some(webhook_secret)) => Some(regnmed_api::StripeCfg {
            secret_key,
            webhook_secret,
            api_base: std::env::var("STRIPE_API_BASE").ok(),
        }),
        (None, None) => None,
        _ => anyhow::bail!(
            "STRIPE_SECRET_KEY og STRIPE_WEBHOOK_SECRET må settes sammen (docs/abonnement.md)"
        ),
    };
    if stripe.is_some() {
        println!("kortskinnen er på (Stripe)");
    }

    // A malformed AGENT_SEALING_KEY stops the process rather than quietly
    // leaving the rail off: a key that is almost right is a
    // configuration mistake someone would otherwise find weeks later,
    // when an agent silently has no model.
    let sealing_key = match std::env::var("AGENT_SEALING_KEY") {
        Ok(hex) => Some(regnmed_core::secret::SealingKey::from_hex(&hex)?),
        Err(_) => None,
    };
    if sealing_key.is_some() {
        println!("hosted agents can store a provider key (AGENT_SEALING_KEY set)");
    }

    // Self-service agents (docs/agent.md §8.6): both credentials or
    // neither, the kortskinnen rule. Without them the portal still
    // creates agents — it just asks for a client_id, as before.
    let regnid = match (
        std::env::var("REGNID_PROVISION_CLIENT_ID").ok(),
        std::env::var("REGNID_PROVISION_SECRET").ok(),
    ) {
        (Some(client_id), Some(client_secret)) => Some(regnmed_api::RegnidCfg {
            client_id,
            client_secret,
            base_url: std::env::var("REGNID_BASE_URL").ok(),
        }),
        (None, None) => None,
        _ => anyhow::bail!(
            "REGNID_PROVISION_CLIENT_ID og REGNID_PROVISION_SECRET må settes sammen \
             (docs/agent.md §8.6)"
        ),
    };
    if regnid.is_some() {
        println!("agents can be created from the portal (regnid provisioning set)");
    }

    // The a-melding rail (docs/lonn.md): on when the Maskinporten client
    // is configured. The scopes are the rail's own, whatever
    // MASKINPORTEN_SCOPES says, and the client defaults to Skatteetatens
    // TEST environment unless AMELDING_URL/DIALOGPORTEN_URL say otherwise.
    let amelding = match std::env::var("MASKINPORTEN_CLIENT_ID") {
        Ok(_) => {
            let mut config = regnmed_gov::maskinporten::MaskinportenConfig::from_env()?;
            config.scopes = regnmed_gov::amelding::SCOPES.into();
            let client = regnmed_gov::amelding::AmeldingClient::from_env();
            println!("a-melding submission is on ({})", client.base_url);
            let events_endpoint = std::env::var("ALTINN_EVENTS_ENDPOINT").ok();
            if let Some(endpoint) = &events_endpoint {
                println!("a-melding feedback arrives as Altinn events at {endpoint}");
            }
            Some(std::sync::Arc::new(regnmed_api::AmeldingRail {
                tokens: regnmed_gov::maskinporten::TokenProvider::new(config),
                client,
                events: regnmed_gov::altinn_events::EventsClient::from_env(),
                events_endpoint,
            }))
        }
        Err(_) => None,
    };

    // The skattekort rail: same client, its own scope, and Skatteetatens
    // TEST environment unless SKATTEKORT_URL says otherwise.
    let tax_card = match std::env::var("MASKINPORTEN_CLIENT_ID") {
        Ok(_) => {
            let mut config = regnmed_gov::maskinporten::MaskinportenConfig::from_env()?;
            config.scopes = regnmed_gov::tax_card::SCOPES.into();
            let client = regnmed_gov::tax_card::TaxCardClient::from_env();
            println!("skattekort fetching is on ({})", client.base_url);
            Some(std::sync::Arc::new(regnmed_api::TaxCardRail {
                tokens: regnmed_gov::maskinporten::TokenProvider::new(config),
                client,
            }))
        }
        Err(_) => None,
    };

    // The aksjonærregisteroppgave rail: same client, its own scope, and
    // Skatteetatens TEST environment unless AKSJONAERREGISTER_URL says otherwise.
    let shareholder_return = match std::env::var("MASKINPORTEN_CLIENT_ID") {
        Ok(_) => {
            let mut config = regnmed_gov::maskinporten::MaskinportenConfig::from_env()?;
            config.scopes = regnmed_gov::shareholder_return::SCOPES.into();
            let client = regnmed_gov::shareholder_return::ShareholderReturnClient::from_env();
            println!(
                "aksjonærregisteroppgave submission is on ({})",
                client.base_url
            );
            Some(std::sync::Arc::new(regnmed_api::ShareholderReturnRail {
                tokens: regnmed_gov::maskinporten::TokenProvider::new(config),
                client,
            }))
        }
        Err(_) => None,
    };

    let app = router(AppState {
        pool,
        verifier,
        mailq,
        rate: Default::default(),
        stripe,
        ops_orgnumber: std::env::var("REGNMED_DRIFT_ORGNR").ok(),
        portal_base: std::env::var("PORTAL_BASE_URL").ok(),
        sealing_key,
        regnid,
        amelding,
        shareholder_return,
        tax_card,
    });

    // BIND_ADDR is authoritative (deploy/ sets it explicitly). PORT is the
    // fallback for dev harnesses that assign a free port.
    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| {
        let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
        format!("127.0.0.1:{port}")
    });
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("regnmed-api listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
