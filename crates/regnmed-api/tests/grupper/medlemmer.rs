//! Membership administration (#53, docs/auth.md).
//!
//! What must hold: an admin can grant and take access, an invitation is
//! redeemed when the address logs in, access through an oppdrag can NOT
//! be changed from here, the company cannot be left without an
//! administrator, and the whole trail is readable afterwards.
//!
//! Krever DATABASE_URL; hopper over ellers.

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
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
                .unwrap(),
        )
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

/// A user who has NOT YET logged in: only the identity the IdP will
/// supply.
///
/// **The e-mail address comes from the token**, not from anything we make
/// up here. `ensure_person` writes it on every login, so an invitation
/// must be addressed to what the IdP actually supplies — that is the same
/// rule as in production, and the test would be worthless without it.
fn kommende(idp: &TestIdp, navn: &str) -> (String, String, String) {
    let sub = format!("{navn}|{}", Uuid::new_v4());
    // Normalised form: that is how it is stored and compared.
    let epost = format!("{}@test.invalid", sub.replace('|', ".")).to_lowercase();
    (sub.clone(), epost, idp.token(&sub, navn))
}

/// The same, but logged in: the person exists in the database.
async fn innlogget(state: &AppState, idp: &TestIdp, navn: &str) -> (Uuid, String) {
    let (sub, _, token) = kommende(idp, navn);
    let id = regnmed_db::ensure_person(&state.pool, &sub, Some(navn), None)
        .await
        .unwrap();
    (id, token)
}

async fn company_with_admin(state: &AppState, idp: &TestIdp) -> (Uuid, Uuid, String) {
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Tilgang AS")
        .await
        .unwrap();
    let (admin_id, admin_token) = innlogget(state, idp, "Admin").await;
    regnmed_db::ensure_company_member(&state.pool, company, admin_id, "admin")
        .await
        .unwrap();
    (company, admin_id, admin_token)
}

/// The whole life cycle: invite an address that does not yet exist, let
/// it log in, and watch the access come into being.
#[tokio::test]
async fn an_invitation_becomes_a_membership_on_login() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, admin) = company_with_admin(&state, &idp).await;

    let (_, epost, ny_token) = kommende(&idp, "Nyansatt");
    let (status, svar) = call(
        &state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &admin,
        // Upper case and spaces must make no difference.
        Some(json!({"epost": format!("  {} ", epost.to_uppercase()), "rolle": "bokforing"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{svar}");

    // The address is normalised, so the invitation reads in lower case.
    let (_, liste) = call(
        &state,
        "GET",
        &format!("/companies/{company}/invitations"),
        &admin,
        None,
    )
    .await;
    assert_eq!(liste["invitasjoner"][0]["epost"], epost);

    // The person does not exist yet — she logs in for the first time.
    let (status, me) = call(&state, "GET", "/me", &ny_token, None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["nye_tilganger"], 1);
    assert_eq!(me["companies"][0]["company_id"], company.to_string());
    assert_eq!(me["companies"][0]["access"], "bokforing");

    // The invitation is used up, not left lying around.
    let (_, liste) = call(
        &state,
        "GET",
        &format!("/companies/{company}/invitations"),
        &admin,
        None,
    )
    .await;
    assert_eq!(liste["invitasjoner"].as_array().unwrap().len(), 0);

    // And it is not redeemed a second time.
    let (_, me) = call(&state, "GET", "/me", &ny_token, None).await;
    assert_eq!(me["nye_tilganger"], 0);
}

/// The answer to an invitation must not reveal whether the address
/// already has a user with us. Otherwise any company admin could look up
/// who is a user on the platform, one attempt at a time.
#[tokio::test]
async fn the_invitation_response_does_not_reveal_whether_the_user_exists() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, admin) = company_with_admin(&state, &idp).await;
    innlogget(&state, &idp, "Finnes").await;

    let (s1, svar1) = call(
        &state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &admin,
        Some(json!({"epost": "finnes@test.invalid", "rolle": "les"})),
    )
    .await;
    let (s2, svar2) = call(
        &state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &admin,
        Some(json!({"epost": "finnes-ikke@test.invalid", "rolle": "les"})),
    )
    .await;

    assert_eq!(s1, s2);
    // Bare id-en skiller svarene.
    assert_eq!(svar1["epost_sendt"], svar2["epost_sendt"]);
    assert_eq!(
        svar1.as_object().unwrap().keys().collect::<Vec<_>>(),
        svar2.as_object().unwrap().keys().collect::<Vec<_>>()
    );
}

/// A company without an administrator is not recoverable without DB
/// access. So the last one cannot demote or remove themselves.
#[tokio::test]
async fn the_last_administrator_cannot_remove_themselves() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, admin_id, admin) = company_with_admin(&state, &idp).await;

    let (status, svar) = call(
        &state,
        "PUT",
        &format!("/companies/{company}/access/{admin_id}"),
        &admin,
        Some(json!({"rolle": "les"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{svar}");
    assert!(
        svar["error"]
            .as_str()
            .unwrap_or_default()
            .contains("uten administrator"),
        "{svar}"
    );

    let (status, _) = call(
        &state,
        "DELETE",
        &format!("/companies/{company}/access/{admin_id}"),
        &admin,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // With a second admin it goes through fine.
    let (nr2, _) = innlogget(&state, &idp, "Nestor").await;
    regnmed_db::ensure_company_member(&state.pool, company, nr2, "admin")
        .await
        .unwrap();
    let (status, svar) = call(
        &state,
        "PUT",
        &format!("/companies/{company}/access/{admin_id}"),
        &admin,
        Some(json!({"rolle": "les"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{svar}");
}

/// Access through an oppdrag is governed by the engagement. An attempt to
/// change it here must say so, not look as though it worked.
#[tokio::test]
async fn oppdrag_access_cannot_be_changed_from_here() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, admin) = company_with_admin(&state, &idp).await;

    let firm = regnmed_db::ensure_firm(&state.pool, &unique_orgnr(), "Byrå AS", "regnskap")
        .await
        .unwrap();
    let (regnskapsforer, _) = innlogget(&state, &idp, "Regnskapsfører").await;
    regnmed_db::ensure_firm_member(&state.pool, firm, regnskapsforer, "ansatt")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(&state.pool, firm, company, "regnskap")
        .await
        .unwrap();

    // The person shows up as a member, but marked as not editable.
    let (_, liste) = call(
        &state,
        "GET",
        &format!("/companies/{company}/access"),
        &admin,
        None,
    )
    .await;
    let via_oppdrag = liste["medlemmer"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["person_id"] == regnskapsforer.to_string())
        .expect("regnskapsføreren skal være med i listen");
    assert_eq!(via_oppdrag["via"], "Byrå AS");
    assert_eq!(via_oppdrag["kan_endres"], false);
    assert_eq!(via_oppdrag["rolle"], "bokforing");

    // And an attempt to change it is refused with an explanation.
    let (status, svar) = call(
        &state,
        "PUT",
        &format!("/companies/{company}/access/{regnskapsforer}"),
        &admin,
        Some(json!({"rolle": "les"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{svar}");
    assert!(
        svar["error"]
            .as_str()
            .unwrap_or_default()
            .contains("oppdrag"),
        "{svar}"
    );
}

/// Only whoever has MEDLEM_ADMIN. A bookkeeper has full write access to
/// the hovedbok and must still not be able to let anybody in.
#[tokio::test]
async fn bokforing_cannot_grant_others_access() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, _) = company_with_admin(&state, &idp).await;
    let (bokforer_id, bokforer) = innlogget(&state, &idp, "Bokfører").await;
    regnmed_db::ensure_company_member(&state.pool, company, bokforer_id, "bokforing")
        .await
        .unwrap();

    for (method, uri, body) in [
        (
            "POST",
            format!("/companies/{company}/invitations"),
            Some(json!({"epost": "x@y.no", "rolle": "admin"})),
        ),
        ("GET", format!("/companies/{company}/access"), None),
        ("GET", format!("/companies/{company}/access/history"), None),
    ] {
        let (status, svar) = call(&state, method, &uri, &bokforer, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}: {svar}");
    }
}

/// The trail answers the question a revisor asks: who granted whom
/// access, and when.
#[tokio::test]
async fn the_trail_shows_who_granted_whom_access() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, admin) = company_with_admin(&state, &idp).await;

    let (sub, epost, token) = kommende(&idp, "Spor");
    call(
        &state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &admin,
        Some(json!({"epost": epost, "rolle": "les"})),
    )
    .await;
    call(&state, "GET", "/me", &token, None).await;
    let person_id = regnmed_db::ensure_person(&state.pool, &sub, None, None)
        .await
        .unwrap();

    call(
        &state,
        "PUT",
        &format!("/companies/{company}/access/{person_id}"),
        &admin,
        Some(json!({"rolle": "bokforing"})),
    )
    .await;
    call(
        &state,
        "DELETE",
        &format!("/companies/{company}/access/{person_id}"),
        &admin,
        None,
    )
    .await;

    let (status, svar) = call(
        &state,
        "GET",
        &format!("/companies/{company}/access/history"),
        &admin,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{svar}");
    let endringer = svar["endringer"].as_array().unwrap();
    // Nyeste først.
    assert_eq!(endringer[0]["endring"], "deaktivert");
    assert_eq!(endringer[0]["utfort_av"], "Admin");
    assert_eq!(endringer[1]["endring"], "rolle_endret");
    assert_eq!(endringer[1]["fra_rolle"], "les");
    assert_eq!(endringer[1]["til_rolle"], "bokforing");
    // The redemption has no actor — the person redeemed it themselves,
    // and who invited them is on the invitation.
    assert_eq!(endringer[2]["endring"], "lagt_til");
    assert_eq!(endringer[2]["kilde"], "invitasjon");
    assert!(endringer[2]["utfort_av"].is_null());

    // And the access is genuinely gone.
    let (_, me) = call(&state, "GET", "/me", &token, None).await;
    assert_eq!(me["companies"].as_array().unwrap().len(), 0);
}

/// A revoked invitation cannot be redeemed.
#[tokio::test]
async fn a_revoked_invitation_grants_no_access() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, admin) = company_with_admin(&state, &idp).await;

    let (_, angret_epost, angret_token) = kommende(&idp, "Angret");
    let (_, svar) = call(
        &state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &admin,
        Some(json!({"epost": angret_epost, "rolle": "admin"})),
    )
    .await;
    let id = svar["invitasjon_id"].as_str().unwrap();

    let (status, _) = call(
        &state,
        "DELETE",
        &format!("/companies/{company}/invitations/{id}"),
        &admin,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, me) = call(&state, "GET", "/me", &angret_token, None).await;
    assert_eq!(me["nye_tilganger"], 0);
    assert_eq!(me["companies"].as_array().unwrap().len(), 0);
}

/// An invitation can point at a custom role that is **deactivated before
/// it is redeemed**. The membership then comes into being with a role
/// that grants nothing: the person enters the company and gets nothing.
///
/// That is chosen, not discovered (docs/auth.md §7). The alternative —
/// refusing the redemption — would have had to explain to the invitee
/// why, i.e. that the company has a deactivated role by that name.
/// Fail-closed is better than leaking, and an admin can assign a
/// different role right away.
#[tokio::test]
async fn an_invitation_to_a_deactivated_role_yields_membership_without_access() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, admin) = company_with_admin(&state, &idp).await;

    let (status, svar) = call(
        &state,
        "POST",
        &format!("/companies/{company}/roles"),
        &admin,
        Some(json!({"navn": "Midlertidig", "rettigheter": ["FAKTURA_LES"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{svar}");
    let role_id = svar["role_id"].as_str().unwrap().to_string();

    let (_, epost, ny_token) = kommende(&idp, "Vikar");
    let (status, svar) = call(
        &state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &admin,
        Some(json!({"epost": epost, "rolle": "Midlertidig"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{svar}");

    // Rollen trekkes tilbake mens invitasjonen ligger ute.
    let (status, _) = call(
        &state,
        "POST",
        &format!("/companies/{company}/roles/{role_id}/deactivate"),
        &admin,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The membership comes into being — the invitation is valid, it was
    // not revoked.
    let (status, me) = call(&state, "GET", "/me", &ny_token, None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["nye_tilganger"], 1);
    assert_eq!(me["companies"][0]["access"], "Midlertidig");

    // Men rollen gir ingenting.
    let (status, _) = call(
        &state,
        "GET",
        &format!("/companies/{company}/invoices"),
        &ny_token,
        None,
    )
    .await;
    assert_ne!(
        status,
        StatusCode::OK,
        "en deaktivert rolle skal ikke gi tilgang, heller ikke via invitasjon"
    );
}
