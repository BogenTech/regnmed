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

    let app = router(AppState {
        pool,
        verifier,
        mailq,
        rate: Default::default(),
        stripe,
        drift_orgnr: std::env::var("REGNMED_DRIFT_ORGNR").ok(),
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
