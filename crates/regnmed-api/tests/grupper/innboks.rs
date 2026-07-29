//! Bilagsinnboks over the web API: a client uploads a document, the
//! regnskapsfører (bokforing via engagement) posts it — voucher,
//! attachment and inbox status land in ONE transaction; a failed
//! posting leaves the document undecided; rejection needs a note;
//! re-deciding is refused; the revisor may look but not decide.
//! Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    content_type: Option<&str>,
    body: Option<Vec<u8>>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
    }
    let response = router(state.clone())
        .oneshot(builder.body(Body::from(body.unwrap_or_default())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn inbox_document_becomes_a_voucher_with_attachment_atomically() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    // Klient (company admin), regnskapsfører (engagement), revisor (les).
    let klient_sub = format!("test|{}", Uuid::new_v4());
    let klient = regnmed_db::ensure_person(&state.pool, &klient_sub, Some("Kari Klient"), None)
        .await
        .unwrap();
    let forer_sub = format!("test|{}", Uuid::new_v4());
    let forer = regnmed_db::ensure_person(&state.pool, &forer_sub, Some("Frida Fører"), None)
        .await
        .unwrap();
    let revisor_sub = format!("test|{}", Uuid::new_v4());
    let revisor = regnmed_db::ensure_person(&state.pool, &revisor_sub, Some("Randi Revisor"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Innboks AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, klient, "admin")
        .await
        .unwrap();
    let byra = regnmed_db::ensure_firm(&state.pool, &unique_orgnr(), "Byrået AS", "regnskap")
        .await
        .unwrap();
    regnmed_db::ensure_firm_member(&state.pool, byra, forer, "ansatt")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(&state.pool, byra, company, "regnskap")
        .await
        .unwrap();
    let revisjonsfirma =
        regnmed_db::ensure_firm(&state.pool, &unique_orgnr(), "Revisjon AS", "revisjon")
            .await
            .unwrap();
    regnmed_db::ensure_firm_member(&state.pool, revisjonsfirma, revisor, "ansatt")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(&state.pool, revisjonsfirma, company, "revisjon")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [("6300", "Leie"), ("1920", "Bank")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let klient_token = idp.token(&klient_sub, "Kari Klient");
    let forer_token = idp.token(&forer_sub, "Frida Fører");
    let revisor_token = idp.token(&revisor_sub, "Randi Revisor");

    // Klienten laster opp kvitteringen.
    let receipt = b"kvittering: husleie juli, 12 500,00".to_vec();
    let (status, uploaded) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox?filename=husleie-juli.pdf"),
        &klient_token,
        Some("application/pdf"),
        Some(receipt.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{uploaded}");
    let document_id = uploaded["document_id"].as_str().unwrap().to_string();

    // A revisor sees it, but does not get to decide.
    let (status, listing) = request(
        &state,
        "GET",
        &format!("/companies/{company}/inbox?status=ny"),
        &revisor_token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["documents"].as_array().unwrap().len(), 1);
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{document_id}/avvis"),
        &revisor_token,
        Some("application/json"),
        Some(json!({"note": "nei"}).to_string().into_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // An unbalanced posting fails — and the document stays 'ny'.
    let bad = json!({
        "journal_code": "GL", "date": "2026-07-01", "description": "Husleie",
        "lines": [
            {"account": "6300", "amount_ore": 12_500_00},
            {"account": "1920", "amount_ore": -12_000_00},
        ],
    });
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{document_id}/bokfor"),
        &forer_token,
        Some("application/json"),
        Some(bad.to_string().into_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, listing) = request(
        &state,
        "GET",
        &format!("/companies/{company}/inbox?status=ny"),
        &forer_token,
        None,
        None,
    )
    .await;
    assert_eq!(
        listing["documents"].as_array().unwrap().len(),
        1,
        "still undecided"
    );

    // Regnskapsføreren bokfører: bilag + vedlegg + status i én transaksjon.
    let good = json!({
        "journal_code": "GL", "date": "2026-07-01", "description": "Husleie juli",
        "lines": [
            {"account": "6300", "amount_ore": 12_500_00},
            {"account": "1920", "amount_ore": -12_500_00},
        ],
    });
    let (status, posted) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{document_id}/bokfor"),
        &forer_token,
        Some("application/json"),
        Some(good.to_string().into_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    let voucher_id = posted["voucher_id"].as_str().unwrap().to_string();

    // The attachment hangs on the bilag with the SAME content hash as the document.
    let attachments =
        regnmed_db::list_attachments(&state.pool, company, Uuid::parse_str(&voucher_id).unwrap())
            .await
            .unwrap();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].filename, "husleie-juli.pdf");
    assert_eq!(
        attachments[0].sha256_hex,
        hex::encode(regnmed_core::hash::sha256(&receipt)),
        "attachment carries the exact uploaded bytes"
    );
    assert_eq!(
        attachments[0].uploaded_by, "Frida Fører",
        "the decision-maker is on record"
    );

    // The status is bokført with a link to the bilag; re-posting is refused.
    let (_, listing) = request(
        &state,
        "GET",
        &format!("/companies/{company}/inbox"),
        &forer_token,
        None,
        None,
    )
    .await;
    let doc = &listing["documents"][0];
    assert_eq!(doc["status"], "bokfort");
    assert_eq!(doc["voucher_id"].as_str().unwrap(), voucher_id);
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{document_id}/bokfor"),
        &forer_token,
        Some("application/json"),
        Some(good.to_string().into_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "already decided");

    // The chain verifies over the new bilag.
    let chain = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(chain.vouchers_checked, 1);

    // Rejection requires a note, and works on a fresh document.
    let (_, uploaded2) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox?filename=uleselig.jpg"),
        &klient_token,
        Some("image/jpeg"),
        Some(b"blur".to_vec()),
    )
    .await;
    let doc2 = uploaded2["document_id"].as_str().unwrap();
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{doc2}/avvis"),
        &forer_token,
        Some("application/json"),
        Some(json!({"note": "  "}).to_string().into_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty note refused");
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{doc2}/avvis"),
        &forer_token,
        Some("application/json"),
        Some(
            json!({"note": "Uleselig — ta nytt bilde"})
                .to_string()
                .into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Fremmede ser ingenting.
    let stranger_sub = format!("test|{}", Uuid::new_v4());
    regnmed_db::ensure_person(&state.pool, &stranger_sub, Some("Fremmed"), None)
        .await
        .unwrap();
    let (status, _) = request(
        &state,
        "GET",
        &format!("/companies/{company}/inbox"),
        &idp.token(&stranger_sub, "Fremmed"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The uploader does not decide what the server says the bytes are (#64).
///
/// This is the attack the hardening exists for: someone with only
/// `BILAG_LAST_OPP` — an `ansatt`, or an allowed e-mail sender — uploads
/// HTML and gets the server to serve it back as HTML on our own origin.
/// Then it runs with the portal's origin, and `nosniff` cannot help,
/// because we would be asserting the dangerous type ourselves.
///
/// The filename is the second half: a quote in it would close the
/// quoted-string in Content-Disposition and let the uploader append
/// header parameters, and CR/LF would end the header line outright.
#[tokio::test]
async fn an_uploaded_html_file_is_neither_served_as_html_nor_trusted_in_the_header() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("laster|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Ola Opplaster"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Herding AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let token = idp.token(&sub, "Ola Opplaster");

    let evil = br#"<script>alert(document.cookie)</script>"#.to_vec();
    let (status, uploaded) = request(
        &state,
        "POST",
        // A quote AND a CRLF in the filename, both uploader-controlled.
        &format!(
            "/companies/{company}/inbox?filename={}",
            urlencoding("evil\".html\r\nX-Injected: 1")
        ),
        &token,
        Some("text/html"),
        Some(evil.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {uploaded}");
    let document_id = uploaded["document_id"].as_str().unwrap();

    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/companies/{company}/inbox/{document_id}/content"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();

    // The bytes come back untouched — the document is evidence and is
    // never rewritten (migration 0015). Only what we SAY about them changes.
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(body.as_ref(), evil.as_slice());

    let ct = headers.get("content-type").unwrap().to_str().unwrap();
    assert_eq!(
        ct, "application/octet-stream",
        "an uploaded text/html must never be asserted as html"
    );
    let cd = headers
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        cd.starts_with("attachment;"),
        "must download, never render: {cd}"
    );
    // The property, not one exact string: nothing in the header may end
    // the line or close the quoted-string early.
    assert!(
        !cd.contains('\r') && !cd.contains('\n'),
        "CRLF survived: {cd}"
    );
    let quoted = cd
        .split_once("filename=\"")
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0;
    assert!(
        !quoted.contains('"') && quoted.starts_with("evil_.html"),
        "the quote must not survive into the quoted-string: {cd}"
    );
    assert_eq!(
        headers.get("x-content-type-options").unwrap(),
        "nosniff",
        "nosniff belongs on every response"
    );
}

/// The headers that hold when a habit slips — asserted on the portal
/// itself, because the CSP is the one that turns a forgotten escape into
/// a console error instead of an intrusion.
#[tokio::test]
async fn the_portal_carries_a_csp_and_every_response_carries_nosniff() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let response = router(state.clone())
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let headers = response.headers();
    assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
    assert_eq!(headers.get("referrer-policy").unwrap(), "no-referrer");

    let csp = headers
        .get("content-security-policy")
        .expect("the SPA must carry a CSP")
        .to_str()
        .unwrap()
        .to_string();
    // The clause that matters: script may not come from inline text, only
    // from our own origin and the one hashed bootstrap script.
    assert!(csp.contains("script-src 'self' 'sha256-"), "{csp}");
    assert!(
        !csp.contains("script-src 'self' 'unsafe-inline'"),
        "unsafe-inline would defeat the whole point: {csp}"
    );
    assert!(csp.contains("object-src 'none'"), "{csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");

    // A JSON endpoint gets nosniff but no CSP — it is meaningless there.
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert!(response.headers().get("content-security-policy").is_none());
}

/// Percent-encodes a query value, so the test can put a quote and a
/// newline in a filename without the request builder rejecting the URI.
fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
