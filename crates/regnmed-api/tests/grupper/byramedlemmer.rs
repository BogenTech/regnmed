//! Byrå membership (#78): invitation → redemption at login → client
//! access through the engagements; role and deactivation guarded by the
//! last-admin rule; everything admin-only; registration first come,
//! first served. Requires DATABASE_URL (skips otherwise).
//!
//! Guard-measurement note (docs/auth.md): the 404-for-non-admin
//! assertions send VALID bodies, so axum's extractors succeed and the
//! status can only come from the guard itself. The bogus-id decision
//! check pins the difference: the same request answers 400 (handler,
//! unknown id) for an admin and 404 (guard) for an ansatt.

use crate::common::{TestIdp, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn request(
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
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = router(state.clone())
        .oneshot(
            builder
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

/// The e-mail the TestIdp bakes into a token for this sub — invitations
/// must be addressed to it for redemption to find them.
fn epost_for(sub: &str) -> String {
    format!("{}@test.invalid", sub.replace('|', "."))
}

#[tokio::test]
async fn byra_members_are_invited_never_coregistered() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    // Founder registers the firm (db level — the API route in front of
    // this is Finanstilsynet-gated and covered in marketplace.rs).
    let grunnlegger_sub = format!("test|{}", Uuid::new_v4());
    let grunnlegger_token = idp.token(&grunnlegger_sub, "Grunnlegger Gro");
    let grunnlegger = regnmed_db::ensure_person(
        &state.pool,
        &grunnlegger_sub,
        Some("Grunnlegger Gro"),
        Some(&epost_for(&grunnlegger_sub)),
    )
    .await
    .unwrap();
    let firm_orgnr = unique_orgnr();
    let firm_id = regnmed_db::create_verified_firm(
        &state.pool,
        &firm_orgnr,
        "BYRÅTEST AS",
        "regnskap",
        "test",
        grunnlegger,
    )
    .await
    .unwrap();

    // First come, first served: a second person registering the same
    // orgnr is refused, not silently made co-admin.
    let annen =
        regnmed_db::ensure_person(&state.pool, &format!("test|{}", Uuid::new_v4()), None, None)
            .await
            .unwrap();
    let feil = regnmed_db::create_verified_firm(
        &state.pool,
        &firm_orgnr,
        "BYRÅTEST AS",
        "regnskap",
        "test",
        annen,
    )
    .await
    .unwrap_err();
    assert!(
        feil.to_string().contains("allerede registrert"),
        "refusal names the reason: {feil}"
    );
    let medlemmer: i64 = sqlx::query_scalar("select count(*) from firm_member where firm_id = $1")
        .bind(firm_id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(medlemmer, 1, "the stranger did not become a member");

    // The founder's arrival is on the record.
    let kilde: String = sqlx::query_scalar(
        "select kilde from firm_member_change where firm_id = $1 and person_id = $2",
    )
    .bind(firm_id)
    .bind(grunnlegger)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(kilde, "registrering");

    // A client company with an open engagement — what membership reaches.
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Klientselskap AS")
        .await
        .unwrap();
    let klientadmin =
        regnmed_db::ensure_person(&state.pool, &format!("test|{}", Uuid::new_v4()), None, None)
            .await
            .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, klientadmin, "admin")
        .await
        .unwrap();
    let request_id =
        regnmed_db::request_engagement(&state.pool, company, firm_id, None, klientadmin)
            .await
            .unwrap();
    regnmed_db::decide_request(&state.pool, firm_id, request_id, grunnlegger, true)
        .await
        .unwrap();

    // The admin invites an ansatt; the invitation stands although no
    // mail rail is configured in tests.
    let ansatt_sub = format!("test|{}", Uuid::new_v4());
    let ansatt_epost = epost_for(&ansatt_sub);
    let (status, svar) = request(
        &state,
        "POST",
        &format!("/firms/{firm_id}/invitations"),
        &grunnlegger_token,
        Some(serde_json::json!({ "epost": ansatt_epost, "rolle": "ansatt" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {svar}");
    assert_eq!(
        svar["epost_sendt"], false,
        "no NATS in tests — said honestly"
    );
    let (status, _) = request(
        &state,
        "POST",
        &format!("/firms/{firm_id}/invitations"),
        &grunnlegger_token,
        Some(serde_json::json!({ "epost": ansatt_epost, "rolle": "ansatt" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "one open invitation per address"
    );

    // The invitee logs in: /me redeems the invitation and the client
    // portfolio is already in the same response.
    let ansatt_token = idp.token(&ansatt_sub, "Ansatt Anne");
    let (status, me) = request(&state, "GET", "/me", &ansatt_token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["nye_tilganger"], 1, "body: {me}");
    let klient = me["companies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["company_id"] == company.to_string())
        .expect("firm membership reaches the client");
    assert_eq!(klient["access"], "bokforing");
    assert_eq!(klient["via"], "BYRÅTEST AS");
    let (_, mine) = request(&state, "GET", "/firms/mine", &ansatt_token, None).await;
    assert_eq!(mine["firms"][0]["role"], "ansatt");

    // Everything administrative is 404 for an ansatt — valid bodies, so
    // the status is the guard's, not an extractor's.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/firms/{firm_id}/invitations"),
        &ansatt_token,
        Some(serde_json::json!({ "epost": "x@y.no", "rolle": "ansatt" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "ansatt cannot invite");
    let (status, _) = request(
        &state,
        "GET",
        &format!("/firms/{firm_id}/access"),
        &ansatt_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "ansatt cannot list members");

    // Engagement decisions are admin-only now that the roles differ:
    // same bogus id, two different answers — 400 is the handler seeing
    // an unknown id, 404 is the guard never letting the ansatt through.
    let bogus = Uuid::new_v4();
    let (status, _) = request(
        &state,
        "POST",
        &format!("/firms/{firm_id}/requests/{bogus}/decision"),
        &grunnlegger_token,
        Some(serde_json::json!({ "accept": true })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "admin reaches the handler");
    let (status, _) = request(
        &state,
        "POST",
        &format!("/firms/{firm_id}/requests/{bogus}/decision"),
        &ansatt_token,
        Some(serde_json::json!({ "accept": true })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "ansatt never reaches it");

    // The last admin can neither demote nor deactivate themselves.
    let (status, svar) = request(
        &state,
        "PUT",
        &format!("/firms/{firm_id}/access/{grunnlegger}"),
        &grunnlegger_token,
        Some(serde_json::json!({ "rolle": "ansatt" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {svar}");
    assert!(
        svar["error"]
            .as_str()
            .unwrap()
            .contains("uten administrator")
    );
    let (status, _) = request(
        &state,
        "DELETE",
        &format!("/firms/{firm_id}/access/{grunnlegger}"),
        &grunnlegger_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "last admin stays");

    // Promote the ansatt, and the founder may step down; the change
    // trail carries every step.
    let ansatt_person: Uuid = sqlx::query_scalar(
        "select person_id from firm_member fm join person p on p.id = fm.person_id
         where fm.firm_id = $1 and p.oidc_sub = $2",
    )
    .bind(firm_id)
    .bind(&ansatt_sub)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/firms/{firm_id}/access/{ansatt_person}"),
        &grunnlegger_token,
        Some(serde_json::json!({ "rolle": "admin" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &state,
        "DELETE",
        &format!("/firms/{firm_id}/access/{grunnlegger}"),
        &grunnlegger_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "another admin exists now");
    let (status, historikk) = request(
        &state,
        "GET",
        &format!("/firms/{firm_id}/access/history"),
        &ansatt_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let endringer: Vec<&str> = historikk["endringer"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["endring"].as_str().unwrap())
        .collect();
    for forventet in ["lagt_til", "rolle_endret", "deaktivert"] {
        assert!(
            endringer.contains(&forventet),
            "missing {forventet}: {endringer:?}"
        );
    }

    // A revoked invitation grants nothing at login.
    let tredje_sub = format!("test|{}", Uuid::new_v4());
    let (status, svar) = request(
        &state,
        "POST",
        &format!("/firms/{firm_id}/invitations"),
        &ansatt_token,
        Some(serde_json::json!({ "epost": epost_for(&tredje_sub), "rolle": "ansatt" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {svar}");
    let invitasjon_id = svar["invitasjon_id"].as_str().unwrap().to_string();
    let (status, _) = request(
        &state,
        "DELETE",
        &format!("/firms/{firm_id}/invitations/{invitasjon_id}"),
        &ansatt_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let tredje_token = idp.token(&tredje_sub, "Tredje Truls");
    let (_, me) = request(&state, "GET", "/me", &tredje_token, None).await;
    assert_eq!(me["nye_tilganger"], 0);
    let (_, mine) = request(&state, "GET", "/firms/mine", &tredje_token, None).await;
    assert_eq!(mine["firms"].as_array().unwrap().len(), 0);
}
