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
    let app = router(AppState { pool, verifier });

    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("regnmed-api listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
