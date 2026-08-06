//! Hovedbok over the web API (docs/hovedbok.md): the kontoplan lists
//! company accounts WITH computed balances plus the standard catalog;
//! accounts are added from the catalog (name resolved) or as custom
//! numbers (name required); the number is permanent but name/active are
//! editable; manual vouchers post through every existing guard; and an
//! active attestering policy closes the manual side door. Requires
//! DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn call(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    let body = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let response = router(state.clone())
        .oneshot(builder.body(body).unwrap())
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

fn konto<'a>(list: &'a serde_json::Value, number: &str) -> &'a serde_json::Value {
    list["kontoer"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["number"] == number)
        .unwrap_or_else(|| panic!("konto {number} in kontoplan"))
}

#[tokio::test]
async fn kontoplan_and_manual_vouchers_end_to_end() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Bo Bokfører"), None)
        .await
        .unwrap();
    let leser_sub = format!("test|{}", Uuid::new_v4());
    let leser = regnmed_db::ensure_person(&state.pool, &leser_sub, Some("Lise Leser"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Hovedbok AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "bokforing")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, leser, "les")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    let token = idp.token(&sub, "Bo Bokfører");
    let leser_token = idp.token(&leser_sub, "Lise Leser");
    let base = format!("/companies/{company}");

    // The catalog rides along even when the company has zero accounts —
    // that is what makes "every code one keystroke away" true on day 1.
    let (status, list) = call(&state, "GET", &format!("{base}/accounts"), &token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(list["standard"].as_array().unwrap().len() > 200);
    assert!(
        list["standard"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["number"] == "1920")
    );

    // Add from the catalog: the standard name is resolved server-side.
    let (status, created) = call(
        &state,
        "POST",
        &format!("{base}/accounts"),
        &token,
        Some(json!({ "number": "1920" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert!(!created["name"].as_str().unwrap().is_empty());

    // A custom number outside the catalog needs its own name...
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/accounts"),
        &token,
        Some(json!({ "number": "1921" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // ...and with one it is a first-class account.
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/accounts"),
        &token,
        Some(json!({ "number": "1921", "name": "Skattetrekkskonto (egen)" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Duplicates are refused, not silently merged.
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/accounts"),
        &token,
        Some(json!({ "number": "1920" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Manual bilag through the new endpoint — with an account that does
    // not exist yet, refused loudly; then a balanced one that posts.
    let (status, body) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-03-01",
            "description": "Feil konto",
            "lines": [
                { "account": "1920", "amount_ore": 5000_00 },
                { "account": "8888", "amount_ore": -5000_00 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, posted) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-03-01",
            "description": "Overføring til skattetrekk",
            "lines": [
                { "account": "1921", "amount_ore": 5000_00 },
                { "account": "1920", "amount_ore": -5000_00 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    assert!(posted["voucher"].as_str().unwrap().starts_with("2026-"));

    // The kontoplan carries computed balances now.
    let (_, list) = call(&state, "GET", &format!("{base}/accounts"), &token, None).await;
    assert_eq!(konto(&list, "1921")["saldo_ore"], 5000_00);
    assert_eq!(konto(&list, "1920")["saldo_ore"], -5000_00);
    assert_eq!(konto(&list, "1921")["posteringer"], 1);

    // Rename is free (the name is master data); the deactivated account
    // refuses NEW postings while history stands.
    let (status, _) = call(
        &state,
        "PUT",
        &format!("{base}/accounts/1921"),
        &token,
        Some(json!({ "name": "Skattetrekk", "active": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = call(&state, "GET", &format!("{base}/accounts"), &token, None).await;
    assert_eq!(konto(&list, "1921")["name"], "Skattetrekk");
    assert_eq!(konto(&list, "1921")["active"], false);
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-03-02",
            "description": "Mot deaktivert konto",
            "lines": [
                { "account": "1921", "amount_ore": 100 },
                { "account": "1920", "amount_ore": -100 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // les reads the kontoplan but changes nothing — VALID bodies, so the
    // guard is what answers (docs/auth.md: a 422 measures nothing).
    let (status, _) = call(
        &state,
        "GET",
        &format!("{base}/accounts"),
        &leser_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/accounts"),
        &leser_token,
        Some(json!({ "number": "6300" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &leser_token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-03-01",
            "description": "Leser prøver seg",
            "lines": [
                { "account": "1920", "amount_ore": 100 },
                { "account": "1921", "amount_ore": -100 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn manual_posting_respects_the_attestering_policy() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Attestert Ane"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Policy AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [("1920", "Bank"), ("6300", "Leie lokale")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_policy(&state.pool, company, true, Some(10_000_00), None, "test")
        .await
        .unwrap();
    let token = idp.token(&sub, "Attestert Ane");
    let base = format!("/companies/{company}");

    // At/over the grense the manual path is CLOSED — attestation lives
    // on inbox documents, and a side door would defeat #47.
    let (status, body) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-03-01",
            "description": "Stor husleie",
            "lines": [
                { "account": "6300", "amount_ore": 10_000_00 },
                { "account": "1920", "amount_ore": -10_000_00 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("attestering"),
        "{body}"
    );

    // Under the grense the manual path stays open.
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-03-01",
            "description": "Liten kostnad",
            "lines": [
                { "account": "6300", "amount_ore": 500_00 },
                { "account": "1920", "amount_ore": -500_00 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn voucher_listing_pages_and_filters_server_side() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Paige Pager"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Sidevis AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "bokforing")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1920", "Bank"),
        ("6800", "Kontorkostnad"),
        ("6300", "Leie"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let token = idp.token(&sub, "Paige Pager");
    let base = format!("/companies/{company}");

    // 24 kontor + 1 leie = 25 vouchers in 2026.
    for i in 1..=24 {
        let (status, _) = call(
            &state,
            "POST",
            &format!("{base}/vouchers"),
            &token,
            Some(json!({
                "journal_code": "GL",
                "date": format!("2026-04-{:02}", (i % 28) + 1),
                "description": format!("Kontorkostnad {i}"),
                "lines": [
                    { "account": "6800", "amount_ore": i * 100 },
                    { "account": "1920", "amount_ore": -(i * 100) },
                ],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-05-01",
            "description": "Husleie mai",
            "lines": [
                { "account": "6300", "amount_ore": 9_000_00 },
                { "account": "1920", "amount_ore": -9_000_00 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Page 1: newest 20 of 25, WITH lines; page 2: the remaining 5.
    let (status, side1) = call(
        &state,
        "GET",
        &format!("{base}/vouchers?lines=true&from=2026-01-01&to=2026-12-31&limit=20&offset=0"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(side1["total"], 25);
    assert_eq!(side1["vouchers"].as_array().unwrap().len(), 20);
    // Newest first: the husleie voucher (posted last) leads, lines attached.
    assert_eq!(side1["vouchers"][0]["description"], "Husleie mai");
    assert_eq!(side1["vouchers"][0]["lines"].as_array().unwrap().len(), 2);
    let (_, side2) = call(
        &state,
        "GET",
        &format!("{base}/vouchers?lines=true&from=2026-01-01&to=2026-12-31&limit=20&offset=20"),
        &token,
        None,
    )
    .await;
    assert_eq!(side2["vouchers"].as_array().unwrap().len(), 5);

    // The filter runs server-side: by description text...
    let (_, treff) = call(
        &state,
        "GET",
        &format!("{base}/vouchers?lines=true&sok=husleie"),
        &token,
        None,
    )
    .await;
    assert_eq!(treff["total"], 1);
    assert_eq!(treff["vouchers"][0]["description"], "Husleie mai");
    // ...and by account number on the LINES, which headers alone can't see.
    let (_, treff) = call(
        &state,
        "GET",
        &format!("{base}/vouchers?sok=6300"),
        &token,
        None,
    )
    .await;
    assert_eq!(treff["total"], 1);

    // Without parameters the old contract stands: headers only, no lines key.
    let (status, alle) = call(&state, "GET", &format!("{base}/vouchers"), &token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(alle["vouchers"].as_array().unwrap().len(), 25);
    assert!(alle["vouchers"][0].get("lines").is_none());
}
