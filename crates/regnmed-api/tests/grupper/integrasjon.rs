//! Maskin-tilgang til API-et (#45): et maskintoken uten grant får
//! ingenting, en admin gir tilgang på et nivå, roboten navngis i
//! bilagets created_by, tilbakekalling virker med én gang, og
//! ratebegrensningen slår inn. Requires DATABASE_URL (skips otherwise).

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
    body: Option<String>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
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
async fn maskintoken_far_bare_det_en_admin_har_gitt() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Aud Admin"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Butikken AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [("1920", "Bank"), ("3000", "Salgsinntekt")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let admin_token = idp.token(&sub, "Aud Admin");
    let base = format!("/companies/{company}");

    // Maskinklienten er bare et subject i tokenet — vår IdP utsteder
    // det, vi utsteder ingen egne nøkler.
    let client_id = format!("nettbutikk-{}", Uuid::new_v4());
    let machine_token = idp.token(&client_id, "");

    // ---- Uten grant finnes selskapet ikke for roboten ----
    let (status, _) = request(
        &state,
        "GET",
        &format!("{base}/vouchers"),
        &machine_token,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "et gyldig token uten grant gir ingen tilgang"
    );

    // ---- Admin gir tilgang på bokføringsnivå ----
    let (status, granted) = request(
        &state,
        "POST",
        &format!("{base}/integrations"),
        &admin_token,
        Some(
            json!({
                "client_id": client_id,
                "navn": "Nettbutikken",
                "kontakt": "drift@nettbutikken.no",
                "access": "bokforing",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{granted}");
    let integration_id = granted["integration_id"].as_str().unwrap().to_string();

    // ---- Nå slipper roboten til, og bilaget navngir den ----
    let (status, posted) = request(
        &state,
        "POST",
        &format!("{base}/inbox?filename=ordre-5512.pdf"),
        &machine_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{posted}",);

    let voucher = json!({
        "journal_code": "GL", "date": "2026-07-25", "description": "Nettbutikksalg 5512",
        "lines": [
            {"account": "1920", "amount_ore": 1_250_00},
            {"account": "3000", "amount_ore": -1_250_00},
        ],
    });
    // Bilaget legges inn via innboksen, som en integrasjon ville gjort:
    // last opp dokumentet, bokfør det.
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{base}/inbox?filename=ordre-5512.pdf"))
                .header("authorization", format!("Bearer {machine_token}"))
                .header("content-type", "application/pdf")
                .body(Body::from("ordrebekreftelse 5512".as_bytes().to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let uploaded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let document_id = uploaded["document_id"].as_str().unwrap().to_string();
    let (status, posted) = request(
        &state,
        "POST",
        &format!("{base}/inbox/{document_id}/bokfor"),
        &machine_token,
        Some(voucher.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{posted}");

    // Revisjonssporet navngir roboten, ikke et anonymt subject.
    let created_by: String = sqlx::query_scalar(
        "select created_by from voucher where company_id = $1 order by voucher_number desc limit 1",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(created_by, "Nettbutikken", "bilaget navngir integrasjonen");

    // ---- Aktiviteten er synlig for selskapet ----
    let (status, log) = request(
        &state,
        "GET",
        &format!("{base}/integrations/log"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{log}");
    let kall = log["kall"].as_array().unwrap();
    assert!(
        kall.iter().any(|k| k["navn"] == "Nettbutikken"
            && k["path"].as_str().unwrap().contains("/bokfor")),
        "de endrende kallene er logget: {log}"
    );
    let (_, listing) = request(
        &state,
        "GET",
        &format!("{base}/integrations"),
        &admin_token,
        None,
    )
    .await;
    let entry = &listing["integrasjoner"][0];
    assert_eq!(entry["navn"], "Nettbutikken");
    assert_eq!(entry["access"], "bokforing");
    assert_eq!(entry["aktiv"], true);
    assert!(
        entry["kall_i_dag"].as_i64().unwrap() >= 3,
        "også lesingene telles: {entry}"
    );

    // ---- En integrasjon kan ikke gi seg selv mer ----
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/integrations"),
        &machine_token,
        Some(json!({"client_id": "enda-en", "navn": "Enda en", "access": "bokforing"}).to_string()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "bokføringstilgang er ikke admin"
    );

    // ---- Tilbakekalling virker med én gang ----
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/integrations/{integration_id}/revoke"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &state,
        "GET",
        &format!("{base}/vouchers"),
        &machine_token,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "valid_to er eksklusiv — tilgangen er borte i dag, ikke i morgen"
    );
    // Historikken står igjen med hvem som trakk den tilbake.
    let (_, listing) = request(
        &state,
        "GET",
        &format!("{base}/integrations"),
        &admin_token,
        None,
    )
    .await;
    let entry = &listing["integrasjoner"][0];
    assert_eq!(entry["aktiv"], false);
    assert_eq!(entry["revoked_by"], "Aud Admin");
}

#[tokio::test]
async fn ratebegrensningen_stopper_en_lopsk_integrasjon() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Aud Admin"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Grense AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let admin_token = idp.token(&sub, "Aud Admin");
    let base = format!("/companies/{company}");

    let client_id = format!("kasse-{}", Uuid::new_v4());
    let machine_token = idp.token(&client_id, "");
    request(
        &state,
        "POST",
        &format!("{base}/integrations"),
        &admin_token,
        Some(json!({"client_id": client_id, "navn": "Kassa", "access": "les"}).to_string()),
    )
    .await;
    // En stram grense for testens skyld.
    sqlx::query("update integration set rate_limit_min = 3 where navn = 'Kassa'")
        .execute(&state.pool)
        .await
        .unwrap();

    let mut siste = StatusCode::OK;
    for _ in 0..6 {
        let (status, _) = request(
            &state,
            "GET",
            &format!("{base}/vouchers"),
            &machine_token,
            None,
        )
        .await;
        siste = status;
        if status == StatusCode::TOO_MANY_REQUESTS {
            break;
        }
    }
    assert_eq!(
        siste,
        StatusCode::TOO_MANY_REQUESTS,
        "budsjettet tar slutt, og API-et sier det tydelig"
    );
    // Mennesket merker ingenting til robotens grense.
    let (status, _) = request(
        &state,
        "GET",
        &format!("{base}/vouchers"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
