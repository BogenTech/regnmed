//! Machine access to the API (#45): a machine token without a grant gets
//! nothing, an admin grants access at one level, the robot is named in
//! the bilag's created_by, revocation takes effect at once, and the rate
//! limit kicks in. Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, gjor_fakturaklar, test_state, unique_orgnr};
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
async fn a_machine_token_gets_only_what_an_admin_granted() {
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
    gjor_fakturaklar(&state.pool, company).await;
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

    // The machine client is only a subject in the token — our IdP issues
    // it, we issue no keys of our own.
    let client_id = format!("nettbutikk-{}", Uuid::new_v4());
    let machine_token = idp.token(&client_id, "");

    // ---- Without a grant the company does not exist for the robot ----
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

    // ---- An admin grants access at bokføring level ----
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

    // ---- Now the robot gets in, and the bilag names it ----
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
    // The bilag is entered through the innboks, as an integration would:
    // upload the document, post it.
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

    // The audit trail names the robot, not an anonymous subject.
    let created_by: String = sqlx::query_scalar(
        "select created_by from voucher where company_id = $1 order by voucher_number desc limit 1",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(created_by, "Nettbutikken", "bilaget navngir integrasjonen");

    // ---- The activity is visible to the company ----
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

    // ---- An integration cannot give itself more ----
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

    // ---- Revocation takes effect at once ----
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
    // The history is left showing who revoked it.
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
async fn the_rate_limit_stops_a_runaway_integration() {
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
    gjor_fakturaklar(&state.pool, company).await;
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
    // The human notices nothing of the robot's limit.
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
