//! The lookups behind the employment form's suggestions: the company's
//! business units from Enhetsregisteret and SSB's occupation catalogue.
//!
//! Requires DATABASE_URL — skips politely otherwise.

use axum::body::Body;
use axum::extract::Query;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use crate::common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
use regnmed_api::{AppState, router};

async fn call(state: &AppState, token: &str, uri: &str) -> (StatusCode, Value) {
    let response = router(state.clone())
        .oneshot(
            Request::get(uri)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn the_form_lookups_answer_members_only_and_survive_a_registry_outage() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let orgnumber = unique_orgnumber();
    let company = regnmed_db::create_company(&state.pool, &orgnumber, "Forslag AS")
        .await
        .unwrap();
    let sub = format!("admin|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Admin"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let member = idp.token(&sub, "Admin");
    let stranger_sub = format!("stranger|{}", Uuid::new_v4());
    regnmed_db::ensure_person(&state.pool, &stranger_sub, Some("Fremmed"), None)
        .await
        .unwrap();
    let stranger = idp.token(&stranger_sub, "Fremmed");

    let parent = orgnumber.clone();
    let registry = Router::new().route(
        "/underenheter",
        get(
            move |Query(q): Query<std::collections::HashMap<String, String>>| async move {
                assert_eq!(q["overordnetEnhet"], parent);
                Json(json!({"_embedded": {"underenheter": [
                    {"organisasjonsnummer": "315286913", "navn": "AVDELING MELØY",
                     "beliggenhetsadresse": {"kommune": "MELØY"}},
                    {"organisasjonsnummer": "315286921", "navn": "NEDLAGT",
                     "nedleggelsesdato": "2025-01-01"},
                ]}}))
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, registry).await });
    unsafe {
        std::env::set_var("BRREG_API_URL", format!("http://{address}"));
    }

    let units = format!("/companies/{company}/business-units");
    let (status, body) = call(&state, &member, &units).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["units"],
        json!([{"orgnumber": "315286913", "name": "AVDELING MELØY", "municipality": "MELØY"}])
    );

    let occupations = format!("/companies/{company}/occupations?q=forhandlingskonsulent");
    let (status, body) = call(&state, &member, &occupations).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["occupations"][0]["code"], "2512101");

    for uri in [&units, &occupations] {
        let (status, _) = call(&state, &stranger, uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }

    // The registry going away empties the suggestions, not the form.
    server.abort();
    let _ = server.await;
    let (status, body) = call(&state, &member, &units).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["units"], json!([]));
}

async fn send(
    state: &AppState,
    token: &str,
    method: &str,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn withholding_set_by_hand_is_validated_and_leaves_a_trail() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let orgnumber = std::iter::repeat_with(unique_orgnumber)
        .find(|n| regnmed_core::orgnumber::is_valid(n))
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &orgnumber, "Trekk AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    let sub = format!("admin|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Lønnsansvarlig"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let token = idp.token(&sub, "Lønnsansvarlig");
    let stranger_sub = format!("stranger|{}", Uuid::new_v4());
    regnmed_db::ensure_person(&state.pool, &stranger_sub, Some("Fremmed"), None)
        .await
        .unwrap();
    let stranger = idp.token(&stranger_sub, "Fremmed");
    let base = format!("/companies/{company}/employees");

    // A table the routine does not know is refused at registration too.
    let new = |table: i32| {
        json!({"fodselsnummer": "20857597940", "navn": "Tabell", "ansatt_fra": "2026-01-01",
               "manedslonn_ore": 4_000_000, "trekk_type": "tabell", "trekk_tabell": table})
    };
    let (status, body) = send(&state, &token, "POST", &base, new(99_999)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("tabellnummer"),
        "{body}"
    );
    let (status, body) = send(&state, &token, "POST", &base, new(9040)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["employee_id"].as_str().unwrap();
    let uri = format!("{base}/{id}/withholding");

    for (bad, why) in [
        (
            json!({"trekk_type": "tabell", "trekk_tabell": 99_999, "reason": "x"}),
            "tabellnummer",
        ),
        (
            json!({"trekk_type": "prosent", "trekk_prosent_bp": 10_001, "reason": "x"}),
            "trekkprosenten",
        ),
        (json!({"trekk_type": "frikort", "reason": "x"}), "trekktype"),
        (
            json!({"trekk_type": "prosent", "trekk_prosent_bp": 3500, "reason": "  "}),
            "hvorfor",
        ),
    ] {
        let (status, body) = send(&state, &token, "PUT", &uri, bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body["error"].as_str().unwrap().contains(why), "{body}");
    }
    let (status, history) = call(&state, &token, &uri).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        history["history"],
        json!([]),
        "a refused change leaves no trace"
    );

    let good = json!({"trekk_type": "prosent", "trekk_prosent_bp": 3500,
                      "trekk_tabell": 8010, "reason": "  Nytt kort fra den ansatte "});
    let (status, _) = send(&state, &stranger, "PUT", &uri, good.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&state, &stranger, &uri).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = send(&state, &token, "PUT", &uri, good).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, list) = call(&state, &token, &base).await;
    let employee = &list["ansatte"][0];
    assert_eq!(employee["trekk_type"], "prosent");
    assert_eq!(employee["trekk_prosent_bp"], 3500);
    assert!(employee["trekk_tabell"].is_null(), "{employee}");

    let (_, history) = call(&state, &token, &uri).await;
    let change = &history["history"][0];
    assert_eq!(change["from"]["trekk_type"], "tabell");
    assert_eq!(change["from"]["trekk_tabell"], 9040);
    assert_eq!(change["to"]["trekk_prosent_bp"], 3500);
    assert_eq!(change["reason"], "Nytt kort fra den ansatte");
    assert_eq!(change["changed_by"], "Lønnsansvarlig");

    // The percentage on a table card withholds holiday pay: saving the same
    // table keeps it, and another table must not inherit it.
    let table = |n: i32| json!({"trekk_type": "tabell", "trekk_tabell": n, "reason": "Rettet"});
    let (status, body) = send(&state, &token, "PUT", &uri, table(9040)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    sqlx::query("update employee set trekk_prosent_bp = 3100 where id = $1::uuid")
        .bind(id)
        .execute(&state.pool)
        .await
        .unwrap();
    let (status, body) = send(&state, &token, "PUT", &uri, table(9040)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, list) = call(&state, &token, &base).await;
    assert_eq!(list["ansatte"][0]["trekk_prosent_bp"], 3100);
    let (status, body) = send(&state, &token, "PUT", &uri, table(8010)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, list) = call(&state, &token, &base).await;
    assert!(list["ansatte"][0]["trekk_prosent_bp"].is_null(), "{list}");

    // The trail is insert-only, also for the application's own role.
    assert!(
        sqlx::query(
            "update employee_withholding_change set reason = 'x' where employee_id = $1::uuid"
        )
        .bind(id)
        .execute(&state.pool)
        .await
        .is_err()
    );
}

#[tokio::test]
async fn setting_withholding_by_hand_answers_a_card_that_was_not_applied() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let orgnumber = std::iter::repeat_with(unique_orgnumber)
        .find(|n| regnmed_core::orgnumber::is_valid(n))
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &orgnumber, "Merknad AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    let sub = format!("admin|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Lønnsansvarlig"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let token = idp.token(&sub, "Lønnsansvarlig");
    let base = format!("/companies/{company}/employees");
    let nin = "19910298057";
    let (status, body) = send(
        &state,
        &token,
        "POST",
        &base,
        json!({"fodselsnummer": nin, "navn": "Uten kort", "ansatt_fra": "2026-01-01",
               "manedslonn_ore": 4_000_000, "trekk_type": "prosent", "trekk_prosent_bp": 5000}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id: Uuid = body["employee_id"].as_str().unwrap().parse().unwrap();

    let order = Uuid::new_v4();
    regnmed_db::tax_card::record_request(&state.pool, order, company, 2026, 1, "Test")
        .await
        .unwrap();
    regnmed_db::tax_card::record_order(
        &state.pool,
        order,
        company,
        2026,
        &format!("BR{}", order.as_u128() % 1_000_000_000),
        None,
        1,
        "Test",
    )
    .await
    .unwrap();
    regnmed_db::tax_card::record_answer(
        &state.pool,
        company,
        order,
        &[regnmed_db::tax_card::ReceivedCard {
            nin: nin.into(),
            result: "ikkeSkattekort".into(),
            card_id: None,
            issued: None,
            raw: json!({}),
            decision: Err("Før trekket for hånd.".into()),
            table_percent_bp: None,
        }],
    )
    .await
    .unwrap();
    let employees = [(id, "Uten kort".to_string())];
    let warnings = regnmed_db::tax_card::unapplied_warnings(&state.pool, company, 2026, &employees)
        .await
        .unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");

    let (status, body) = send(
        &state,
        &token,
        "PUT",
        &format!("{base}/{id}/withholding"),
        json!({"trekk_type": "tabell", "trekk_tabell": 9040, "reason": "Kort levert på papir"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let warnings = regnmed_db::tax_card::unapplied_warnings(&state.pool, company, 2026, &employees)
        .await
        .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
}
