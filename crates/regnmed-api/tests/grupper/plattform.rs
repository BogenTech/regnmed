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

use crate::common::{TestIdp, gi_partene_adresse, gjor_fakturaklar, test_state, unique_orgnr};
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
    gjor_fakturaklar(&state.pool, company).await;
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
    gi_partene_adresse(&state.pool, company).await;
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

/// The console's numbers: the overview is open to support (the same
/// aggregates the lists already show), while the per-company abonnement
/// list is systemadmin territory like the customer registers. A fresh
/// company with no coverage shows up in its trial — computed by the one
/// status rule, never stored.
#[tokio::test]
async fn the_overview_is_shared_but_subscriptions_are_systemadmin_territory() {
    let Some((state, idp, company)) = setup().await else {
        return;
    };
    // A stranger probes nothing: 404 on both, like the rest of /platform.
    let (_, _, fremmed) = person(&state, &idp, "Fremmed").await;
    for uri in ["/platform/overview", "/platform/subscriptions"] {
        assert_eq!(
            status(&state, "GET", uri, &fremmed, "").await,
            StatusCode::NOT_FOUND,
            "{uri} must not exist for a non-platform person"
        );
    }

    let (_, support) = platform_person(&state, &idp, "support").await;
    let (kode, svar) = json_call(&state, "GET", "/platform/overview", &support, "").await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    assert!(svar["selskaper"].as_i64().unwrap() >= 1);
    assert!(svar["brukere"].as_i64().unwrap() >= 1);
    assert!(
        svar["abonnement"].is_object(),
        "the status distribution must be present: {svar}"
    );
    assert_eq!(
        status(&state, "GET", "/platform/subscriptions", &support, "").await,
        StatusCode::FORBIDDEN,
        "billing per company is systemadmin territory"
    );

    let (_, sysadmin) = platform_person(&state, &idp, "systemadmin").await;
    let (kode, svar) = json_call(&state, "GET", "/platform/subscriptions", &sysadmin, "").await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    let rad = svar["abonnementer"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["company_id"] == company.to_string())
        .expect("the fresh company must be listed")
        .clone();
    // Created moments ago, no coverage row: the trial is running.
    assert_eq!(rad["status"], "prove", "{rad}");
    assert!(rad["plan"].is_null());
    assert!(
        rad.get("saldo_ore").is_none(),
        "billing status only — no balances on the platform path"
    );
}

/// The back office: systemadmin edits master data, memberships and the
/// abonnement relationship on the customer's behalf — support looks but
/// does not touch, the change log names the platform, and the guards
/// that protect a company from its own admins protect it from the
/// platform too.
#[tokio::test]
async fn the_back_office_edits_master_data_and_coverage_but_support_does_not() {
    let Some((state, idp, company)) = setup().await else {
        return;
    };
    let (_, support) = platform_person(&state, &idp, "support").await;
    let (_, sysadmin) = platform_person(&state, &idp, "systemadmin").await;

    // Support sees the drill-down (same master data as the lists) but
    // every mutation below is systemadmin territory.
    assert_eq!(
        status(
            &state,
            "GET",
            &format!("/platform/companies/{company}"),
            &support,
            ""
        )
        .await,
        StatusCode::OK
    );
    for (method, uri, body) in [
        (
            "PUT",
            format!("/platform/companies/{company}/settings"),
            r#"{"address":"x"}"#,
        ),
        (
            "POST",
            format!("/platform/companies/{company}/subscription"),
            r#"{"plan":"standard","note":"sak"}"#,
        ),
        (
            "POST",
            format!("/platform/companies/{company}/subscription/end"),
            "",
        ),
        (
            "PUT",
            "/platform/settings".to_string(),
            r#"{"ikonstil":"emoji"}"#,
        ),
    ] {
        assert_eq!(
            status(&state, method, &uri, &support, body).await,
            StatusCode::FORBIDDEN,
            "{method} {uri} must be systemadmin territory"
        );
    }

    // Master data edit lands where the company's own admin reads it.
    let (kode, _) = json_call(
        &state,
        "PUT",
        &format!("/platform/companies/{company}/settings"),
        &sysadmin,
        r#"{"address":"Plattformgata 1","email":"post@kunde.invalid"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK);
    let (_, detalj) = json_call(
        &state,
        "GET",
        &format!("/platform/companies/{company}"),
        &sysadmin,
        "",
    )
    .await;
    assert_eq!(detalj["settings"]["address"], "Plattformgata 1");

    // Coverage by hand: refuses without an active-status check bypass,
    // carries the mandatory note, and ending it flips the status off
    // aktiv again.
    let (kode, svar) = json_call(
        &state,
        "POST",
        &format!("/platform/companies/{company}/subscription"),
        &sysadmin,
        r#"{"plan":"standard","note":"supportsak 42"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    let (_, detalj) = json_call(
        &state,
        "GET",
        &format!("/platform/companies/{company}"),
        &sysadmin,
        "",
    )
    .await;
    assert_eq!(detalj["abonnement"]["status"], "aktiv");
    assert!(
        detalj["abonnement"]["dekning"][0]["note"]
            .as_str()
            .unwrap()
            .contains("supportsak 42"),
        "the reason must be on the row: {detalj}"
    );
    // A second opening while active is refused.
    let (kode, _) = json_call(
        &state,
        "POST",
        &format!("/platform/companies/{company}/subscription"),
        &sysadmin,
        r#"{"plan":"standard","note":"dobbelt"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::BAD_REQUEST);
    let (kode, _) = json_call(
        &state,
        "POST",
        &format!("/platform/companies/{company}/subscription/end"),
        &sysadmin,
        "",
    )
    .await;
    assert_eq!(kode, StatusCode::OK);
    // The row is closed. The STATUS stays aktiv until the exclusive
    // valid_to passes — coverage opened today ends tomorrow at the
    // earliest ("the shortest truthful coverage is one day"), so
    // asserting on the row rather than the status is the honest check.
    let (_, detalj) = json_call(
        &state,
        "GET",
        &format!("/platform/companies/{company}"),
        &sysadmin,
        "",
    )
    .await;
    assert!(
        !detalj["abonnement"]["dekning"][0]["valid_to"].is_null(),
        "the open row must be closed: {detalj}"
    );
}

/// Membership deactivation from the platform: kilde='plattform' in the
/// company's own log, access gone on the next /me — and the last active
/// admin cannot be deactivated even by the platform (no orphaned
/// companies; the nødprosedyre exists for the opposite problem).
#[tokio::test]
async fn platform_deactivation_is_logged_and_cannot_orphan_a_company() {
    let Some((state, idp, company)) = setup().await else {
        return;
    };
    let (_, sysadmin) = platform_person(&state, &idp, "systemadmin").await;

    let admin_sub = format!("admin|{}", Uuid::new_v4());
    let admin_id = regnmed_db::ensure_person(&state.pool, &admin_sub, Some("Eneadmin"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, admin_id, "admin")
        .await
        .unwrap();

    // The only active admin: refused.
    let (kode, svar) = json_call(
        &state,
        "DELETE",
        &format!("/platform/companies/{company}/members/{admin_id}"),
        &sysadmin,
        "",
    )
    .await;
    assert_eq!(kode, StatusCode::BAD_REQUEST, "{svar}");

    // An ordinary member: deactivated, logged as plattform, access gone.
    let (medlem_id, _, medlem_token) = person(&state, &idp, "Medlem").await;
    regnmed_db::ensure_company_member(&state.pool, company, medlem_id, "les")
        .await
        .unwrap();
    let (kode, _) = json_call(
        &state,
        "DELETE",
        &format!("/platform/companies/{company}/members/{medlem_id}"),
        &sysadmin,
        "",
    )
    .await;
    assert_eq!(kode, StatusCode::OK);
    let (_, me) = json_call(&state, "GET", "/me", &medlem_token, "").await;
    assert!(
        me["companies"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["company_id"] != company.to_string()),
        "access must be gone: {me}"
    );
    let kilde: String = sqlx::query(
        "select kilde from company_member_change
         where company_id = $1 and person_id = $2 and endring = 'deaktivert'",
    )
    .bind(company)
    .bind(medlem_id)
    .fetch_one(&state.pool)
    .await
    .unwrap()
    .get("kilde");
    assert_eq!(kilde, "plattform");

    // Restore works and is also the platform's doing.
    let (kode, _) = json_call(
        &state,
        "POST",
        &format!("/platform/companies/{company}/members/{medlem_id}/restore"),
        &sysadmin,
        "",
    )
    .await;
    assert_eq!(kode, StatusCode::OK);
}

/// The global icon style: validated, systemadmin-set, and served to the
/// whole platform through the unauthenticated /portal-config.
#[tokio::test]
async fn the_icon_style_is_validated_and_served_platform_wide() {
    let Some((state, idp, _company)) = setup().await else {
        return;
    };
    let (_, sysadmin) = platform_person(&state, &idp, "systemadmin").await;

    let (kode, svar) = json_call(
        &state,
        "PUT",
        "/platform/settings",
        &sysadmin,
        r#"{"ikonstil":"comic-sans"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::BAD_REQUEST, "{svar}");

    let (kode, _) = json_call(
        &state,
        "PUT",
        "/platform/settings",
        &sysadmin,
        r#"{"ikonstil":"kraftig"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK);

    // portal-config is public — no bearer at all.
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/portal-config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let config: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(config["ikonstil"], "kraftig");

    // Leave the shared dev database on the default so a test run does
    // not restyle everyone's portal.
    let (kode, _) = json_call(
        &state,
        "PUT",
        "/platform/settings",
        &sysadmin,
        r#"{"ikonstil":"linje"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK);
}
