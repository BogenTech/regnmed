//! Den nye portalen (#76): Svelte 5 + Vite, kompilert til ui/portal/dist
//! som SJEKKES INN og embeddes her — Rust-bygget (også kryssbygget i
//! build-images.sh) trenger aldri Node. Serveres under /ny til alle
//! seksjoner har paritet; da flippes /.
//!
//! Vite gir innholdshashede filnavn (assets/index-XXXX.js), så filene
//! leses med include_dir i stedet for én include_str! per fil.

use axum::extract::Path;
use axum::http::header;
use axum::response::{Html, IntoResponse, Response};
use include_dir::{Dir, include_dir};

static DIST: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../ui/portal/dist");

fn index_html() -> &'static str {
    DIST.get_file("index.html")
        .map(|f| f.contents_utf8().unwrap_or(""))
        .unwrap_or("")
}

/// /ny, /ny/ og /ny/callback: alltid appen — rutingen skjer i hashen,
/// og callback-adressen må lande i appen for å fullføre PKCE-flyten.
pub async fn index() -> Html<&'static str> {
    Html(index_html())
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("webmanifest") => "application/manifest+json",
        Some("map") => "application/json",
        _ => "application/octet-stream",
    }
}

pub async fn asset(Path(path): Path<String>) -> Response {
    match DIST.get_file(&path) {
        Some(file) => (
            [
                (header::CONTENT_TYPE, content_type(&path)),
                // Filnavnet bærer innholdshashen, så innholdet bak en
                // gitt adresse endrer seg aldri — trygt å cache lenge.
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            file.contents(),
        )
            .into_response(),
        // Ukjent sti under /ny: appen selv (SPA-fallback), aldri 404 —
        // en gammel bokmerket adresse skal lande i appen, ikke i feil.
        None => Html(index_html()).into_response(),
    }
}
