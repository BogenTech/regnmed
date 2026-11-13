//! `regnmed-ocr`: POST /read with a JPEG or PNG body, get
//! `{"text": "..."}` back. Cluster-internal; the API calls it when
//! `OCR_URL` is set and carries on without suggestions when it is not.
//!
//! One picture at a time: a second request waits rather than doubling the
//! memory the models use while they work (docs/frugality.md).

use std::sync::Arc;

use anyhow::Context;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::{get, post},
};
use regnmed_ocr::Reader;
use tokio::sync::Semaphore;

struct App {
    reader: Reader,
    one_at_a_time: Arc<Semaphore>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dir = std::env::var("OCR_MODELS_DIR").unwrap_or_else(|_| "/app/ocr-models".into());
    let reader = Reader::load(std::path::Path::new(&dir))?;
    let app = Arc::new(App {
        reader,
        one_at_a_time: Arc::new(Semaphore::new(1)),
    });
    let router = Router::new()
        .route("/read", post(read))
        .route("/healthz", get(|| async { "ok" }))
        // Receipts are at most 15 MB everywhere else in regnmed.
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .with_state(app);
    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .context(addr.clone())?;
    eprintln!("regnmed-ocr listening on {addr}, models from {dir}");
    axum::serve(listener, router).await?;
    Ok(())
}

async fn read(
    State(app): State<Arc<App>>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // The permit moves INTO the blocking task: a caller that gives up
    // (the API times out at 30 s) drops this future, but the read goes on,
    // and the next one must wait for it -- two reads at once is twice the
    // memory.
    let turn = app
        .one_at_a_time
        .clone()
        .acquire_owned()
        .await
        .expect("never closed");
    let worker = app.clone();
    let text = tokio::task::spawn_blocking(move || {
        let _turn = turn;
        worker.reader.read(&body)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, format!("{e:#}")))?;
    Ok(Json(serde_json::json!({ "text": text })))
}
