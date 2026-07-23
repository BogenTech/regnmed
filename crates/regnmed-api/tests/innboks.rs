//! Bilagsinnboks over the web API: a client uploads a document, the
//! regnskapsfører (bokforing via engagement) posts it — voucher,
//! attachment and inbox status land in ONE transaction; a failed
//! posting leaves the document undecided; rejection needs a note;
//! re-deciding is refused; the revisor may look but not decide.
//! Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
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

    // Revisor ser den, men får ikke bestemme.
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

    // En ubalansert bokføring feiler — og dokumentet forblir 'ny'.
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

    // Vedlegget henger på bilaget med SAMME innholdshash som dokumentet.
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

    // Statusen er bokført med kobling til bilaget; re-bokføring avvises.
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

    // Kjeden verifiserer over det nye bilaget.
    let chain = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(chain.vouchers_checked, 1);

    // Avvisning krever notat, og virker på et nytt dokument.
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
