//! Platform roles (docs/auth.md §8): the bounded exception, and its
//! fences.
//!
//! What must not break, in order of importance: (1) a platform role
//! reaches NO company ledger — the trust story's "who at the vendor can
//! read the client's books? no one" stays true; (2) every /platform call
//! is logged and the log is readable by the company it concerned;
//! (3) an ordinary company admin is a stranger to /platform; (4)
//! revocation is immediate; (5) support may add memberships but not
//! change them, and never sees customer registers.

use chrono::{Duration, Utc};
use sqlx::Row;

use crate::common::{TestIdp, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn json_call(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let kode = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        kode,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

async fn status(state: &AppState, method: &str, uri: &str, bearer: &str, body: &str) -> StatusCode {
    json_call(state, method, uri, bearer, body).await.0
}

/// A fresh person with an e-mail, so platform grants (which go by
/// address) can find them.
async fn person(state: &AppState, idp: &TestIdp, navn: &str) -> (Uuid, String, String) {
    let sub = format!("plattform|{}", Uuid::new_v4());
    let epost = format!("{}@test.invalid", sub.replace('|', "."));
    let id = regnmed_db::ensure_person(&state.pool, &sub, Some(navn), Some(&epost))
        .await
        .unwrap();
    (id, epost, idp.token(&sub, navn))
}

/// A person holding an active platform role, granted directly in the
/// database (the CLI bootstrap path).
async fn platform_person(state: &AppState, idp: &TestIdp, rolle: &str) -> (Uuid, String) {
    let (id, _, token) = person(state, idp, rolle).await;
    let til = (Utc::now() + Duration::days(30)).date_naive();
    regnmed_db::grant_platform_role(&state.pool, id, rolle, til, "test", None)
        .await
        .unwrap();
    (id, token)
}

async fn setup() -> Option<(AppState, TestIdp, Uuid)> {
    let idp = TestIdp::new();
    let state = test_state(&idp).await?;
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Plattformtest AS")
        .await
        .unwrap();
    Some((state, idp, company))
}

/// The strongest ordinary role there is remains a complete stranger to
/// /platform — and a platform person without membership remains a
/// complete stranger to every company, ledger first. Both directions of
/// the same fence.
#[tokio::test]
async fn a_company_admin_is_a_stranger_to_the_platform_and_vice_versa() {
    let Some((state, idp, company)) = setup().await else {
        return;
    };
    let sub = format!("admin|{}", Uuid::new_v4());
    let admin_id = regnmed_db::ensure_person(&state.pool, &sub, Some("Admin"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, admin_id, "admin")
        .await
        .unwrap();
    let admin = idp.token(&sub, "Admin");

    for uri in [
        "/platform/companies",
        "/platform/firms",
        "/platform/users",
        "/platform/customers",
        "/platform/members",
    ] {
        assert_eq!(
            status(&state, "GET", uri, &admin, "").await,
            StatusCode::NOT_FOUND,
            "company admin must be a stranger to {uri}"
        );
    }
    let (_, me) = json_call(&state, "GET", "/me", &admin, "").await;
    assert!(me["plattform"].is_null());

    // The other direction is the load-bearing one: a platform systemadmin
    // gets 404 on the company's ledger, master data and administration —
    // the platform path grants NO company access.
    let (_, sysadmin) = platform_person(&state, &idp, "systemadmin").await;
    for uri in [
        // Valid query params on purpose: an invalid query would 400 in the
        // extractor before the guard is ever asked (docs/auth.md's trap).
        format!("/companies/{company}/reports/saldobalanse?from=2026-01-01&to=2026-12-31"),
        format!("/companies/{company}/vouchers"),
        format!("/companies/{company}/parties"),
        format!("/companies/{company}/invoices"),
        format!("/companies/{company}/access"),
        format!("/companies/{company}/platform-access"),
    ] {
        assert_eq!(
            status(&state, "GET", &uri, &sysadmin, "").await,
            StatusCode::NOT_FOUND,
            "a platform role must not reach {uri}"
        );
    }
    let (_, me) = json_call(&state, "GET", "/me", &sysadmin, "").await;
    assert_eq!(me["plattform"]["rolle"], "systemadmin");
    assert_eq!(me["companies"].as_array().unwrap().len(), 0);
}

/// Support: master data yes, customer registers and platform-user
/// administration no. New memberships yes, changes to existing ones no.
#[tokio::test]
async fn support_sees_master_data_but_neither_customers_nor_member_admin() {
    let Some((state, idp, company)) = setup().await else {
        return;
    };
    let (_, support) = platform_person(&state, &idp, "support").await;

    assert_eq!(
        status(&state, "GET", "/platform/companies", &support, "").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&state, "GET", "/platform/users", &support, "").await,
        StatusCode::OK
    );
    for (method, uri) in [
        ("GET", "/platform/customers"),
        ("GET", "/platform/members"),
        ("POST", "/platform/members"),
    ] {
        let body = if method == "POST" {
            r#"{"epost":"x@test.invalid","rolle":"support","valid_to":"2030-01-01","notat":"n"}"#
        } else {
            ""
        };
        assert_eq!(
            status(&state, method, uri, &support, body).await,
            StatusCode::FORBIDDEN,
            "{method} {uri} is systemadmin territory"
        );
    }

    // A new membership: allowed, and the target actually gains access.
    let (target, _, target_token) = person(&state, &idp, "Ny Bruker").await;
    let (kode, svar) = json_call(
        &state,
        "POST",
        &format!("/platform/users/{target}/companies/{company}"),
        &support,
        r#"{"rolle":"les"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    let (_, me) = json_call(&state, "GET", "/me", &target_token, "").await;
    assert_eq!(me["companies"][0]["company_id"], company.to_string());

    // Changing the existing membership: refused for support, allowed for
    // systemadmin — "only System Admins set roles on every user".
    let (kode, svar) = json_call(
        &state,
        "POST",
        &format!("/platform/users/{target}/companies/{company}"),
        &support,
        r#"{"rolle":"bokforing"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::BAD_REQUEST);
    assert!(svar["error"].as_str().unwrap().contains("systemadmin"));

    let (_, sysadmin) = platform_person(&state, &idp, "systemadmin").await;
    let (kode, _) = json_call(
        &state,
        "POST",
        &format!("/platform/users/{target}/companies/{company}"),
        &sysadmin,
        r#"{"rolle":"bokforing"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK);

    // The company's own history names the platform as the source.
    let kilder: Vec<String> = sqlx::query(
        "select kilde from company_member_change where company_id = $1 and person_id = $2",
    )
    .bind(company)
    .bind(target)
    .fetch_all(&state.pool)
    .await
    .unwrap()
    .iter()
    .map(|r| r.get("kilde"))
    .collect();
    assert!(!kilder.is_empty());
    assert!(kilder.iter().all(|k| k == "plattform"));
}

/// Every /platform call leaves a log row, and the rows that concern a
/// company are readable by that company's admin — and by no lesser role.
#[tokio::test]
async fn platform_access_is_logged_and_visible_to_the_company() {
    let Some((state, idp, company)) = setup().await else {
        return;
    };
    let sub = format!("admin|{}", Uuid::new_v4());
    let admin_id = regnmed_db::ensure_person(&state.pool, &sub, Some("Admin"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, admin_id, "admin")
        .await
        .unwrap();
    let admin = idp.token(&sub, "Admin");
    let les_sub = format!("les|{}", Uuid::new_v4());
    let les_id = regnmed_db::ensure_person(&state.pool, &les_sub, Some("Les"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, les_id, "les")
        .await
        .unwrap();
    let les = idp.token(&les_sub, "Les");

    let (support_id, support) = platform_person(&state, &idp, "support").await;
    let (target, _, _) = person(&state, &idp, "Innsatt").await;
    let (kode, _) = json_call(
        &state,
        "POST",
        &format!("/platform/users/{target}/companies/{company}"),
        &support,
        r#"{"rolle":"ansatt"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK);

    let (kode, svar) = json_call(
        &state,
        "GET",
        &format!("/companies/{company}/platform-access"),
        &admin,
        "",
    )
    .await;
    assert_eq!(kode, StatusCode::OK);
    let innsyn = svar["innsyn"].as_array().unwrap();
    assert_eq!(
        innsyn.len(),
        1,
        "the assignment call must be visible: {svar}"
    );
    assert_eq!(innsyn[0]["rolle"], "support");
    assert_eq!(innsyn[0]["method"], "POST");

    // `les` has membership but not member administration: 403, not 404.
    assert_eq!(
        status(
            &state,
            "GET",
            &format!("/companies/{company}/platform-access"),
            &les,
            ""
        )
        .await,
        StatusCode::FORBIDDEN
    );

    // Even a refused call is logged: the middleware writes the row
    // before the handler decides, so a probe shows up as an attempt.
    let telle = "select count(*) as n from platform_access_log where person_id = $1";
    let n_before: i64 = sqlx::query(telle)
        .bind(support_id)
        .fetch_one(&state.pool)
        .await
        .unwrap()
        .get("n");
    assert_eq!(
        status(&state, "GET", "/platform/customers", &support, "").await,
        StatusCode::FORBIDDEN
    );
    let n_after: i64 = sqlx::query(telle)
        .bind(support_id)
        .fetch_one(&state.pool)
        .await
        .unwrap()
        .get("n");
    assert_eq!(n_after, n_before + 1, "a refused call still leaves its row");
}

/// Grant through the API, then revoke: the role works, and stops working
/// the moment it is ended — exclusive valid_to, checked per request.
#[tokio::test]
async fn revocation_of_a_platform_role_is_immediate() {
    let Some((state, idp, _company)) = setup().await else {
        return;
    };
    let (_, sysadmin) = platform_person(&state, &idp, "systemadmin").await;
    let (_, epost, token) = person(&state, &idp, "Ny Support").await;

    let til = (Utc::now() + Duration::days(14)).date_naive();
    let (kode, svar) = json_call(
        &state,
        "POST",
        "/platform/members",
        &sysadmin,
        &format!(r#"{{"epost":"{epost}","rolle":"support","valid_to":"{til}","notat":"sak 42"}}"#),
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    let member_id = svar["id"].as_str().unwrap().to_string();
    assert_eq!(
        status(&state, "GET", "/platform/companies", &token, "").await,
        StatusCode::OK
    );

    let (kode, _) = json_call(
        &state,
        "DELETE",
        &format!("/platform/members/{member_id}"),
        &sysadmin,
        "",
    )
    .await;
    assert_eq!(kode, StatusCode::OK);
    assert_eq!(
        status(&state, "GET", "/platform/companies", &token, "").await,
        StatusCode::NOT_FOUND,
        "revocation must take effect on the next request"
    );
}

/// The customer register a systemadmin sees is master data with the
/// owning company named — and nothing else travels with it.
#[tokio::test]
async fn the_customer_register_names_the_company_and_carries_no_ledger() {
    let Some((state, idp, company)) = setup().await else {
        return;
    };
    // A unique name (no spaces — it goes into the URI raw): the search
    // is capped at 100 rows, and the test database accumulates
    // customers, so a common name falls off the page.
    let navn = format!("Kunden-{}", Uuid::new_v4());
    let (party_id, _party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", &navn, None, None)
            .await
            .unwrap();
    let (_, sysadmin) = platform_person(&state, &idp, "systemadmin").await;
    let (kode, svar) = json_call(
        &state,
        "GET",
        &format!("/platform/customers?sok={navn}"),
        &sysadmin,
        "",
    )
    .await;
    assert_eq!(kode, StatusCode::OK);
    let kunder = svar["kunder"].as_array().unwrap();
    let kunde = kunder
        .iter()
        .find(|k| k["party_id"] == party_id.to_string())
        .expect("the seeded customer must be findable");
    assert_eq!(kunde["selskap"]["company_id"], company.to_string());
    assert_eq!(kunde["selskap"]["navn"], "Plattformtest AS");
    assert!(
        kunde.get("saldo_ore").is_none(),
        "master data only — no balances on the platform path"
    );
}
