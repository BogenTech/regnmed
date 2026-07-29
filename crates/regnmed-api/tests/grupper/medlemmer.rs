//! Medlemsadministrasjon (#53, docs/auth.md).
//!
//! Det som må stemme: en admin kan gi og ta tilgang, en invitasjon
//! løses inn når adressen logger inn, tilgang gjennom et oppdrag kan
//! IKKE endres herfra, selskapet kan ikke bli stående uten
//! administrator, og hele sporet er lesbart etterpå.
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

/// En bruker som ENNÅ IKKE har logget inn: bare identiteten IdP-en vil
/// oppgi.
///
/// **E-postadressen kommer fra tokenet**, ikke fra noe vi finner på her.
/// `ensure_person` skriver den fra hver innlogging, så en invitasjon må
/// stiles til adressen IdP-en faktisk oppgir — det er den samme regelen
/// som gjelder i produksjon, og testen ville vært verdiløs uten den.
fn kommende(idp: &TestIdp, navn: &str) -> (String, String, String) {
    let sub = format!("{navn}|{}", Uuid::new_v4());
    // Normalisert form: det er slik den lagres og sammenlignes.
    let epost = format!("{}@test.invalid", sub.replace('|', ".")).to_lowercase();
    (sub.clone(), epost, idp.token(&sub, navn))
}

/// Samme, men logget inn: personen finnes i databasen.
async fn innlogget(state: &AppState, idp: &TestIdp, navn: &str) -> (Uuid, String) {
    let (sub, _, token) = kommende(idp, navn);
    let id = regnmed_db::ensure_person(&state.pool, &sub, Some(navn), None)
        .await
        .unwrap();
    (id, token)
}

async fn selskap_med_admin(state: &AppState, idp: &TestIdp) -> (Uuid, Uuid, String) {
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Tilgang AS")
        .await
        .unwrap();
    let (admin_id, admin_token) = innlogget(state, idp, "Admin").await;
    regnmed_db::ensure_company_member(&state.pool, company, admin_id, "admin")
        .await
        .unwrap();
    (company, admin_id, admin_token)
}

/// Hele livsløpet: inviter en adresse som ennå ikke finnes, la den
/// logge inn, og se at tilgangen blir til.
#[tokio::test]
async fn invitasjon_blir_til_medlemskap_ved_innlogging() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, admin) = selskap_med_admin(&state, &idp).await;

    let (_, epost, ny_token) = kommende(&idp, "Nyansatt");
    let (status, svar) = call(
        &state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &admin,
        // Store bokstaver og mellomrom skal ikke gjøre en forskjell.
        Some(json!({"epost": format!("  {} ", epost.to_uppercase()), "rolle": "bokforing"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{svar}");

    // Adressen normaliseres, så invitasjonen står på små bokstaver.
    let (_, liste) = call(
        &state,
        "GET",
        &format!("/companies/{company}/invitations"),
        &admin,
        None,
    )
    .await;
    assert_eq!(liste["invitasjoner"][0]["epost"], epost);

    // Personen finnes ikke ennå — hun logger inn for første gang.
    let (status, me) = call(&state, "GET", "/me", &ny_token, None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["nye_tilganger"], 1);
    assert_eq!(me["companies"][0]["company_id"], company.to_string());
    assert_eq!(me["companies"][0]["access"], "bokforing");

    // Invitasjonen er brukt opp, ikke liggende.
    let (_, liste) = call(
        &state,
        "GET",
        &format!("/companies/{company}/invitations"),
        &admin,
        None,
    )
    .await;
    assert_eq!(liste["invitasjoner"].as_array().unwrap().len(), 0);

    // Og den løses ikke inn en gang til.
    let (_, me) = call(&state, "GET", "/me", &ny_token, None).await;
    assert_eq!(me["nye_tilganger"], 0);
}

/// Svaret på en invitasjon skal ikke røpe om adressen alt har en bruker
/// hos oss. Ellers kunne enhver selskapsadmin slå opp hvem som er
/// bruker på plattformen, ett forsøk om gangen.
#[tokio::test]
async fn invitasjonssvaret_rper_ikke_om_brukeren_finnes() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, admin) = selskap_med_admin(&state, &idp).await;
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

/// Et selskap uten administrator er ikke gjenopprettelig uten
/// DB-tilgang. Derfor kan den siste ikke degradere eller fjerne seg
/// selv.
#[tokio::test]
async fn siste_administrator_kan_ikke_fjerne_seg_selv() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, admin_id, admin) = selskap_med_admin(&state, &idp).await;

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

    // Med en admin nummer to går det fint.
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

/// Tilgang gjennom et oppdrag styres av engasjementet. Et forsøk på å
/// endre den her skal si fra, ikke se ut som om det virket.
#[tokio::test]
async fn oppdragstilgang_kan_ikke_endres_herfra() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, admin) = selskap_med_admin(&state, &idp).await;

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

    // Vedkommende vises som medlem, men merket som ikke-endrbar.
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

    // Og forsøk på å endre den avvises med en forklaring.
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

/// Bare den som har MEDLEM_ADMIN. En bokfører har full skrivetilgang til
/// hovedboken og skal likevel ikke kunne slippe inn noen.
#[tokio::test]
async fn bokforing_kan_ikke_gi_andre_tilgang() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, _) = selskap_med_admin(&state, &idp).await;
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

/// Sporet svarer på spørsmålet en revisor stiller: hvem ga hvem
/// tilgang, og når.
#[tokio::test]
async fn sporet_viser_hvem_som_ga_hvem_tilgang() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, admin) = selskap_med_admin(&state, &idp).await;

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
    // Innløsningen har ingen utfører — personen løste den inn selv, og
    // hvem som inviterte står på invitasjonen.
    assert_eq!(endringer[2]["endring"], "lagt_til");
    assert_eq!(endringer[2]["kilde"], "invitasjon");
    assert!(endringer[2]["utfort_av"].is_null());

    // Og tilgangen er faktisk borte.
    let (_, me) = call(&state, "GET", "/me", &token, None).await;
    assert_eq!(me["companies"].as_array().unwrap().len(), 0);
}

/// En tilbakekalt invitasjon kan ikke løses inn.
#[tokio::test]
async fn tilbakekalt_invitasjon_gir_ingen_tilgang() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, _, admin) = selskap_med_admin(&state, &idp).await;

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
