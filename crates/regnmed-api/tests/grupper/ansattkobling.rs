//! Ansatt ↔ portalbruker (docs/lonn.md, migration 0050).
//!
//! The link decides who may read a payslip and whose hours become
//! timelønn, so what must not break is the guards: the invitation path
//! links the redeeming person in the same transaction as the
//! membership; a conflicting link never blocks a login; manual linking
//! refuses relink-without-unlink, persons who are already another
//! employee, and machine identities; and every change leaves its row in
//! the insert-only trail.

use sqlx::Row;

use crate::common::{TestIdp, gjor_fakturaklar, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

/// Tenor test numbers (Skatteetaten's synthetic range) — valid check
/// digits, no real person.
const FNR: [&str; 3] = ["26829398612", "08888797336", "25927898821"];

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

async fn setup() -> Option<(AppState, TestIdp, Uuid, String)> {
    let idp = TestIdp::new();
    let state = test_state(&idp).await?;
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Koblingstest AS")
        .await
        .unwrap();
    gjor_fakturaklar(&state.pool, company).await;
    let sub = format!("admin|{}", Uuid::new_v4());
    let admin_id = regnmed_db::ensure_person(&state.pool, &sub, Some("Admin"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, admin_id, "admin")
        .await
        .unwrap();
    let token = idp.token(&sub, "Admin");
    Some((state, idp, company, token))
}

async fn ny_ansatt(state: &AppState, company: Uuid, admin: &str, navn: &str, fnr: &str) -> Uuid {
    let (kode, svar) = json_call(
        state,
        "POST",
        &format!("/companies/{company}/employees"),
        admin,
        &format!(
            r#"{{"fodselsnummer":"{fnr}","navn":"{navn}","ansatt_fra":"2026-01-01",
                 "manedslonn_ore":5000000,"trekk_type":"prosent","trekk_prosent_bp":3000}}"#
        ),
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    svar["employee_id"].as_str().unwrap().parse().unwrap()
}

async fn ansatt_person_json(
    state: &AppState,
    company: Uuid,
    admin: &str,
    employee_id: Uuid,
) -> serde_json::Value {
    let (kode, svar) = json_call(
        state,
        "GET",
        &format!("/companies/{company}/employees"),
        admin,
        "",
    )
    .await;
    assert_eq!(kode, StatusCode::OK);
    svar["ansatte"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == employee_id.to_string())
        .expect("employee must be listed")["person"]
        .clone()
}

/// The primary path: the invitation carries the employee, and the
/// redeeming login links them — same transaction as the membership,
/// audited with kilde='invitasjon' and no utfort_av (the person did it
/// themselves; the inviter stands on the invitation).
#[tokio::test]
async fn an_invitation_carrying_an_employee_links_on_first_login() {
    let Some((state, idp, company, admin)) = setup().await else {
        return;
    };
    let employee = ny_ansatt(&state, company, &admin, "Kari Kobling", FNR[0]).await;

    // The future user's address is derived the way TestIdp derives it,
    // so the /me redemption matches.
    let sub = format!("ansatt|{}", Uuid::new_v4());
    let epost = format!("{}@test.invalid", sub.replace('|', "."));
    let (kode, svar) = json_call(
        &state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &admin,
        &format!(r#"{{"epost":"{epost}","rolle":"ansatt","employee_id":"{employee}"}}"#),
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");

    // Only one open invitation may promise an employee.
    let (kode, svar) = json_call(
        &state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &admin,
        &format!(r#"{{"epost":"annen@test.invalid","rolle":"ansatt","employee_id":"{employee}"}}"#),
    )
    .await;
    assert_eq!(kode, StatusCode::BAD_REQUEST);
    assert!(svar["error"].as_str().unwrap().contains("åpen invitasjon"));

    // First login: membership AND link arrive together.
    let token = idp.token(&sub, "Kari Kobling");
    let (kode, me) = json_call(&state, "GET", "/me", &token, "").await;
    assert_eq!(kode, StatusCode::OK);
    assert_eq!(me["nye_tilganger"], 1);
    assert_eq!(me["companies"][0]["company_id"], company.to_string());

    let person = ansatt_person_json(&state, company, &admin, employee).await;
    assert_eq!(person["person_id"], me["person_id"]);

    let kilde: String = sqlx::query(
        "select kilde from employee_link_change where employee_id = $1 and endring = 'koblet'",
    )
    .bind(employee)
    .fetch_one(&state.pool)
    .await
    .unwrap()
    .get("kilde");
    assert_eq!(kilde, "invitasjon");
}

/// A person who is already another employee here redeems an invitation
/// carrying a second employee: the membership stands, the link is
/// skipped, and the employee remains VISIBLY unlinked — the login never
/// breaks over an invitation mistake.
#[tokio::test]
async fn a_conflicting_link_never_blocks_the_login() {
    let Some((state, idp, company, admin)) = setup().await else {
        return;
    };
    let forste = ny_ansatt(&state, company, &admin, "Allerede Ansatt", FNR[0]).await;
    let andre = ny_ansatt(&state, company, &admin, "Ny Ansatt", FNR[1]).await;

    let sub = format!("ansatt|{}", Uuid::new_v4());
    let epost = format!("{}@test.invalid", sub.replace('|', "."));
    let person_id = regnmed_db::ensure_person(&state.pool, &sub, Some("Q"), Some(&epost))
        .await
        .unwrap();
    let admin_person: Uuid = sqlx::query_scalar(
        "select person_id from company_member where company_id = $1 and role = 'admin'",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    regnmed_db::lonn::link_ansatt_person(&state.pool, company, forste, person_id, admin_person)
        .await
        .unwrap();

    let (kode, _) = json_call(
        &state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &admin,
        &format!(r#"{{"epost":"{epost}","rolle":"ansatt","employee_id":"{andre}"}}"#),
    )
    .await;
    assert_eq!(kode, StatusCode::OK);

    let token = idp.token(&sub, "Q");
    let (kode, me) = json_call(&state, "GET", "/me", &token, "").await;
    assert_eq!(kode, StatusCode::OK, "the login must survive the conflict");
    assert_eq!(me["nye_tilganger"], 1, "membership is still redeemed");

    let person = ansatt_person_json(&state, company, &admin, andre).await;
    assert!(
        person.is_null(),
        "the second employee must stay visibly unlinked, not silently mislinked"
    );
}

/// The manual path holds its guards regardless of what the UI sends.
#[tokio::test]
async fn manual_linking_refuses_relink_conflicts_and_machines() {
    let Some((state, idp, company, admin)) = setup().await else {
        return;
    };
    let employee = ny_ansatt(&state, company, &admin, "Manuell Ansatt", FNR[2]).await;
    let link_uri = format!("/companies/{company}/employees/{employee}/link");

    let medlem_sub = format!("les|{}", Uuid::new_v4());
    let medlem = regnmed_db::ensure_person(&state.pool, &medlem_sub, Some("Medlem"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, medlem, "les")
        .await
        .unwrap();

    let (kode, svar) = json_call(
        &state,
        "POST",
        &link_uri,
        &admin,
        &format!(r#"{{"person_id":"{medlem}"}}"#),
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");

    // Relinking to someone else without unlinking first is refused.
    let annen = regnmed_db::ensure_person(
        &state.pool,
        &format!("les|{}", Uuid::new_v4()),
        Some("Annen"),
        None,
    )
    .await
    .unwrap();
    let (kode, svar) = json_call(
        &state,
        "POST",
        &link_uri,
        &admin,
        &format!(r#"{{"person_id":"{annen}"}}"#),
    )
    .await;
    assert_eq!(kode, StatusCode::BAD_REQUEST);
    assert!(svar["error"].as_str().unwrap().contains("koble fra først"));

    // The same person cannot be a second employee in the company.
    let tvilling = ny_ansatt(&state, company, &admin, "Tvilling", FNR[0]).await;
    let (kode, svar) = json_call(
        &state,
        "POST",
        &format!("/companies/{company}/employees/{tvilling}/link"),
        &admin,
        &format!(r#"{{"person_id":"{medlem}"}}"#),
    )
    .await;
    assert_eq!(kode, StatusCode::BAD_REQUEST);
    assert!(svar["error"].as_str().unwrap().contains("annen ansatt"));

    // A machine identity is never a lønnsmottaker.
    let robot = regnmed_db::ensure_person(
        &state.pool,
        &format!("robot|{}", Uuid::new_v4()),
        Some("Robot"),
        None,
    )
    .await
    .unwrap();
    sqlx::query("update person set kind = 'integrasjon' where id = $1")
        .bind(robot)
        .execute(&state.pool)
        .await
        .unwrap();
    let (kode, svar) = json_call(
        &state,
        "POST",
        &format!("/companies/{company}/employees/{tvilling}/link"),
        &admin,
        &format!(r#"{{"person_id":"{robot}"}}"#),
    )
    .await;
    assert_eq!(kode, StatusCode::BAD_REQUEST);
    assert!(svar["error"].as_str().unwrap().contains("integrasjon"));

    // Unlink, then the history names both events.
    let (kode, _) = json_call(&state, "DELETE", &link_uri, &admin, "").await;
    assert_eq!(kode, StatusCode::OK);
    let (kode, svar) = json_call(&state, "GET", &format!("{link_uri}/history"), &admin, "").await;
    assert_eq!(kode, StatusCode::OK);
    let historikk = svar["historikk"].as_array().unwrap();
    assert_eq!(historikk.len(), 2);
    assert_eq!(historikk[0]["endring"], "frakoblet");
    assert_eq!(historikk[1]["endring"], "koblet");
    assert_eq!(historikk[1]["kilde"], "admin");
    assert_eq!(historikk[1]["utfort_av"], "Admin");
}
