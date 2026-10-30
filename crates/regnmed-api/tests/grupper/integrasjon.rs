//! Machine access to the API (#45): a machine token without a grant gets
//! nothing, an admin grants access at one level, the robot is named in
//! the bilag's created_by, revocation takes effect at once, and the rate
//! limit kicks in. Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
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
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Butikken AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
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
    // The log row is written from a detached task so that logging never
    // sits on the request path (auth.rs), which makes this read
    // eventually consistent. Poll rather than assume: CI once lost the
    // race on a loaded postgres while the same test passed locally.
    let mut log = serde_json::Value::Null;
    for _ in 0..50 {
        let (status, body) = request(
            &state,
            "GET",
            &format!("{base}/integrations/log"),
            &admin_token,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        log = body;
        if log["kall"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["navn"] == "Nettbutikken" && k["path"].as_str().unwrap().contains("/bokfor"))
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let call = log["kall"].as_array().unwrap();
    assert!(
        call.iter().any(|k| k["navn"] == "Nettbutikken"
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

    // ---- An agent has a name and a face (docs/agent.md §8.1) ----
    assert!(
        listing["farger"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "primary"),
        "fargene serveres fra koden som validerer dem: {listing}"
    );
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/integrations/{integration_id}"),
        &admin_token,
        Some(
            json!({"navn": "Kari Bokfører", "avatar_farge": "primary", "avatar_ikon": "bilag"})
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, listing) = request(
        &state,
        "GET",
        &format!("{base}/integrations"),
        &admin_token,
        None,
    )
    .await;
    let entry = &listing["integrasjoner"][0];
    assert_eq!(entry["navn"], "Kari Bokfører");
    assert_eq!(entry["avatar_farge"], "primary");
    assert_eq!(entry["avatar_ikon"], "bilag");
    assert_eq!(
        entry["access"], "bokforing",
        "ansiktet rører ikke tilgangen"
    );

    // A colour the renderer has no class for must not reach the column:
    // the server's list is the gate, the portal's table only draws.
    let (status, body) = request(
        &state,
        "PUT",
        &format!("{base}/integrations/{integration_id}"),
        &admin_token,
        Some(json!({"navn": "Kari Bokfører", "avatar_farge": "bg-[url(evil)]"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Renaming an agent is administering machine access, so the machine
    // itself may not do it — not even to itself.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/integrations/{integration_id}"),
        &machine_token,
        Some(json!({"navn": "Jeg Bestemmer Selv"}).to_string()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "en maskin døper ikke om seg selv"
    );

    // ---- The customer's own LLM provider (docs/agent.md §8) ----
    let (_, llm) = request(
        &state,
        "GET",
        &format!("{base}/integrations/{integration_id}/llm"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(llm["tilgjengelig"], true, "testnøkkelen er satt: {llm}");
    assert!(llm["oppsett"].is_null(), "ingen leverandør ennå: {llm}");

    let (status, body) = request(
        &state,
        "PUT",
        &format!("{base}/integrations/{integration_id}/llm"),
        &admin_token,
        Some(
            json!({
                "leverandor": "openrouter",
                "modell": "qwen/qwen3-8b",
                "api_nokkel": "sk-or-hemmelig-verdi"
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, llm) = request(
        &state,
        "GET",
        &format!("{base}/integrations/{integration_id}/llm"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(llm["oppsett"]["leverandor"], "openrouter");
    assert_eq!(llm["oppsett"]["base_url"], "https://openrouter.ai/api/v1");
    assert_eq!(llm["oppsett"]["modell"], "qwen/qwen3-8b");
    assert_eq!(llm["oppsett"]["nokkel_lagret"], true);
    // THE rule: the key has no way back out. Checked against the whole
    // response rather than a field, because a leak would arrive as a
    // field nobody thought about.
    assert!(
        !llm.to_string().contains("sk-or-hemmelig-verdi"),
        "nøkkelen lekket ut igjen: {llm}"
    );

    // Nor is it in the database in the clear.
    let lagret: Vec<u8> = sqlx::query_scalar(
        "select nokkel from agent_llm where integration_id = $1 order by created_at desc limit 1",
    )
    .bind(integration_id.parse::<uuid::Uuid>().unwrap())
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert!(
        !String::from_utf8_lossy(&lagret).contains("sk-or-hemmelig-verdi"),
        "nøkkelen ligger i klartekst i databasen"
    );

    // http:// is refused: the ledger and the key travel over it.
    let (status, body) = request(
        &state,
        "PUT",
        &format!("{base}/integrations/{integration_id}/llm"),
        &admin_token,
        Some(
            json!({
                "leverandor": "annet",
                "base_url": "http://gateway.invalid/v1",
                "modell": "m",
                "api_nokkel": "x"
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // The machine may not choose its own provider or pay with someone
    // else's key: that is administering machine access.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/integrations/{integration_id}/llm"),
        &machine_token,
        Some(json!({"leverandor": "openai", "modell": "m", "api_nokkel": "x"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Turning it off keeps the record of what it was.
    let (status, _) = request(
        &state,
        "DELETE",
        &format!("{base}/integrations/{integration_id}/llm"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, llm) = request(
        &state,
        "GET",
        &format!("{base}/integrations/{integration_id}/llm"),
        &admin_token,
        None,
    )
    .await;
    assert!(llm["oppsett"].is_null(), "modellen er av: {llm}");
    let rader: i64 = sqlx::query_scalar("select count(*) from agent_llm where integration_id = $1")
        .bind(integration_id.parse::<uuid::Uuid>().unwrap())
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(rader, 2, "sporet beholder leverandøren som var valgt");

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
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Grense AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
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

    let mut last = StatusCode::OK;
    for _ in 0..6 {
        let (status, _) = request(
            &state,
            "GET",
            &format!("{base}/vouchers"),
            &machine_token,
            None,
        )
        .await;
        last = status;
        if status == StatusCode::TOO_MANY_REQUESTS {
            break;
        }
    }
    assert_eq!(
        last,
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

/// #101: an agent is a USER. The owner may give it a real role —
/// `admin` included — and then the only thing it still cannot do is
/// change who has access.
///
/// **Seen failing**: with the `maskin` filter removed from
/// `Tilgang::har`, the two FORBIDDEN assertions below return 200 and
/// the robot invites members into the company. That is what this test
/// is here for; the migration assertion only proves the point of the
/// revision (the old two-value grant could not express it at all).
#[tokio::test]
async fn an_agent_with_admin_may_migrate_but_never_widen_access() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Eier Eiersen"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Agentselskapet AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let admin_token = idp.token(&sub, "Eier Eiersen");
    let base = format!("/companies/{company}");

    let client_id = format!("regnskapsagent-{}", Uuid::new_v4());
    let agent_token = idp.token(&client_id, "");

    // ---- A role name that is not a role is refused, with the choices ----
    let (status, error) = request(
        &state,
        "POST",
        &format!("{base}/integrations"),
        &admin_token,
        Some(
            json!({"client_id": client_id, "navn": "Agenten", "access": "regnskapsforer"})
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let message = error["error"].as_str().unwrap();
    assert!(
        message.contains("bokforing") && message.contains("admin"),
        "feilen sier hva man kan velge: {message}"
    );

    // ---- The owner grants admin, as #101 decided she may ----
    let (status, granted) = request(
        &state,
        "POST",
        &format!("{base}/integrations"),
        &admin_token,
        Some(json!({"client_id": client_id, "navn": "Agenten", "access": "admin"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{granted}");

    // ---- MIGRERING_ADMIN works: this is the whole point of #101 ----
    let (status, response) = request(
        &state,
        "GET",
        &format!("{base}/import/saft/log"),
        &agent_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let (status, response) = request(
        &state,
        "POST",
        &format!("{base}/import/saft/analyze"),
        &agent_token,
        Some("ikke en saft-fil".to_string()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "agenten slipper gjennom vakten og feiler på INNHOLDET, ikke på tilgangen: {response}"
    );

    // ---- But access control is not hers, whatever the role says ----
    for (method, uri, body) in [
        (
            "POST",
            format!("{base}/integrations"),
            Some(json!({"client_id": "enda-en", "navn": "Enda en", "access": "les"}).to_string()),
        ),
        (
            "POST",
            format!("{base}/invitations"),
            Some(json!({"epost": "ny@example.com", "rolle": "admin"}).to_string()),
        ),
    ] {
        let (status, response) = request(&state, method, &uri, &agent_token, body).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{uri} styrer hvem som har tilgang og kan ikke delegeres til en maskin: {response}"
        );
    }

    // ---- /me must not promise what the guard will refuse ----
    let (status, me) = request(&state, "GET", "/me", &agent_token, None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    let company = me["companies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["company_id"].as_str() == Some(&company.to_string()))
        .expect("agenten ser selskapet");
    let rights: Vec<&str> = company["rettigheter"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert!(rights.contains(&"MIGRERING_ADMIN"), "{rights:?}");
    for nektet in [
        "MEDLEM_ADMIN",
        "OPPDRAG_ADMIN",
        "INTEGRASJON_ADMIN",
        "SELSKAP_ADMIN",
    ] {
        assert!(
            !rights.contains(&nektet),
            "{nektet} skal ikke stå i /me for en maskin: {rights:?}"
        );
    }
}

/// One call creates the whole agent, and the role can be changed from
/// the same place it is shown (docs/agent.md §8.6).
///
/// Provisioning is off in tests (`regnid: None`), so this exercises the
/// path that does not depend on the IdP being up: a client_id the
/// caller supplies. What the test is really for is the two things the
/// portal now depends on — that the extra instructions and the face
/// survive one POST, and that a role change leaves a dated trail
/// instead of editing a grant.
#[tokio::test]
async fn an_agent_is_created_in_one_call_and_re_roled_without_losing_the_trail() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Aud Admin"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Byrået AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let admin_token = idp.token(&sub, "Aud Admin");
    let base = format!("/companies/{company}");

    // Without provisioning configured the endpoint must SAY so rather
    // than inventing an identity — a customer on such an installation
    // has to be told what to do instead.
    let (status, body) = request(
        &state,
        "POST",
        &format!("{base}/agents"),
        &admin_token,
        Some(json!({"navn": "Uten id", "access": "les"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("client_id"),
        "feilen må si hva som mangler: {body}"
    );

    let client_id = format!("agent-{}", Uuid::new_v4().simple());
    let (status, body) = request(
        &state,
        "POST",
        &format!("{base}/agents"),
        &admin_token,
        Some(
            json!({
                "client_id": client_id,
                "navn": "Kari Bokfører",
                "access": "bokforing",
                "system_prompt": "Strøm bokføres på 6340. Spør Aud ved tvil.",
                "avatar_farge": "primary",
                "avatar_ikon": "bilag",
                "leverandor": "openrouter",
                "modell": "google/gemini-2.5-flash-lite",
                "api_nokkel": "sk-test-ikke-ekte",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["klar"], true, "modell og nøkkel i samme kall: {body}");
    assert_eq!(
        body["advarsler"].as_array().unwrap().len(),
        0,
        "ingen halvferdige steg: {body}"
    );
    let integration_id = body["integration_id"].as_str().unwrap().to_string();

    let (_, listing) = request(
        &state,
        "GET",
        &format!("{base}/integrations"),
        &admin_token,
        None,
    )
    .await;
    let entry = &listing["integrasjoner"][0];
    assert_eq!(entry["navn"], "Kari Bokfører");
    assert_eq!(entry["access"], "bokforing");
    assert_eq!(entry["avatar_ikon"], "bilag");
    assert_eq!(
        entry["system_prompt"],
        "Strøm bokføres på 6340. Spør Aud ved tvil."
    );
    assert_eq!(entry["llm"]["modell"], "google/gemini-2.5-flash-lite");

    // The key went in sealed and has no route back out: the listing
    // says there is a model, never what pays for it.
    assert!(
        !listing.to_string().contains("sk-test-ikke-ekte"),
        "nøkkelen må ikke kunne leses ut: {listing}"
    );

    // ---- Rollen er sikkerhetsgrensen, så den kan endres ----
    let (status, body) = request(
        &state,
        "PUT",
        &format!("{base}/integrations/{integration_id}/rolle"),
        &admin_token,
        Some(json!({"access": "les"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // A grant is immutable, so this must be revoke PLUS a new grant:
    // two rows, and the old role still true for the day it applied.
    let rader: Vec<(String, Option<chrono::NaiveDate>)> = sqlx::query_as(
        "select access, valid_to from integration_grant
         where company_id = $1 order by valid_from, access",
    )
    .bind(company)
    .fetch_all(&state.pool)
    .await
    .unwrap();
    assert_eq!(rader.len(), 2, "sporet beholdes: {rader:?}");
    let active: Vec<&(String, Option<chrono::NaiveDate>)> =
        rader.iter().filter(|r| r.1.is_none()).collect();
    assert_eq!(active.len(), 1, "én levende tilgang: {rader:?}");
    assert_eq!(active[0].0, "les");
    assert!(
        rader.iter().any(|r| r.0 == "bokforing" && r.1.is_some()),
        "den gamle rollen står med sluttdato: {rader:?}"
    );

    // After a role change the SAME agent appears twice in the listing
    // — once active, once ended — so every row must carry a key of its
    // own. Keyed on the integration, the portal rendered duplicate keys,
    // and an uncaught error in a Svelte template freezes the whole
    // component (CLAUDE.md point 68): the card went dead, not wrong.
    let (_, listing) = request(
        &state,
        "GET",
        &format!("{base}/integrations"),
        &admin_token,
        None,
    )
    .await;
    let rader = listing["integrasjoner"].as_array().unwrap();
    assert_eq!(rader.len(), 2, "begge grantene står i listen: {listing}");
    let nokler: std::collections::HashSet<&str> = rader
        .iter()
        .map(|r| r["grant_id"].as_str().unwrap())
        .collect();
    assert_eq!(nokler.len(), 2, "hver rad har sin egen nøkkel: {listing}");
    assert_eq!(
        rader
            .iter()
            .map(|r| r["integration_id"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        1,
        "det er samme agent: {listing}"
    );

    // The same role twice is a no-op worth refusing: otherwise every
    // save would add a row and the trail would stop meaning anything.
    let (status, body) = request(
        &state,
        "PUT",
        &format!("{base}/integrations/{integration_id}/rolle"),
        &admin_token,
        Some(json!({"access": "les"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // And a name that is not a role gets nowhere, as on the grant.
    let (status, body) = request(
        &state,
        "PUT",
        &format!("{base}/integrations/{integration_id}/rolle"),
        &admin_token,
        Some(json!({"access": "sjef"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Instructions are editable, and clearing them is a real action.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/integrations/{integration_id}"),
        &admin_token,
        Some(json!({"navn": "Kari Bokfører", "system_prompt": ""}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, listing) = request(
        &state,
        "GET",
        &format!("{base}/integrations"),
        &admin_token,
        None,
    )
    .await;
    assert!(
        listing["integrasjoner"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["system_prompt"].is_null()),
        "tom tekst fjerner instruksene: {listing}"
    );
}
