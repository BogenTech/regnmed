//! Attestering (#47): with an active policy, an innboks bilag above the
//! amount threshold requires an approved attestering before posting,
//! whoever attested cannot post it themselves, a rejected attestering
//! stops the posting, bilag BELOW the threshold go straight through,
//! payment lists must be approved by someone other than their creator,
//! and utlegg claims cannot be approved by the submitter. The trail is
//! insert-only and is read by a revisor. Requires DATABASE_URL (skips
//! otherwise).

use crate::common::{TestIdp, test_state, unique_orgnr};
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
    content_type: Option<&str>,
    body: Option<Vec<u8>>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
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

async fn upload(state: &AppState, company: Uuid, token: &str, name: &str) -> String {
    let (status, uploaded) = request(
        state,
        "POST",
        &format!("/companies/{company}/inbox?filename={name}"),
        token,
        Some("application/pdf"),
        Some(format!("kvittering {name}").into_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{uploaded}");
    uploaded["document_id"].as_str().unwrap().to_string()
}

fn post_body(belop: i64) -> Vec<u8> {
    json!({
        "journal_code": "GL", "date": "2026-07-10", "description": "Innkjøp",
        "lines": [
            {"account": "6300", "amount_ore": belop},
            {"account": "2400", "amount_ore": -belop, "party_no": "70001"},
        ],
    })
    .to_string()
    .into_bytes()
}

#[tokio::test]
async fn attestering_requires_four_eyes_for_posting_and_payment() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    // Managing director (admin, attestant) and regnskapsfører (bokføring).
    let leder_sub = format!("test|{}", Uuid::new_v4());
    let leder = regnmed_db::ensure_person(&state.pool, &leder_sub, Some("Lise Leder"), None)
        .await
        .unwrap();
    let forer_sub = format!("test|{}", Uuid::new_v4());
    let forer = regnmed_db::ensure_person(&state.pool, &forer_sub, Some("Frida Fører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Kontroll AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, leder, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, forer, "bokforing")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("6300", "Leie"),
        ("2400", "Leverandørgjeld"),
        ("1920", "Bank"),
        ("2910", "Mellomregning"),
        ("7790", "Annen kostnad"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "2400", Some("leverandor"))
        .await
        .unwrap();
    let (leverandor_id, _) = regnmed_db::create_party(
        &state.pool,
        company,
        "leverandor",
        "Grossisten AS",
        None,
        Some("70001"),
    )
    .await
    .unwrap();
    regnmed_db::update_party_contact(
        &state.pool,
        company,
        leverandor_id,
        None,
        None,
        Some("86011117947"),
    )
    .await
    .unwrap();
    sqlx::query("update company set bank_account = '86011117947' where id = $1")
        .bind(company)
        .execute(&state.pool)
        .await
        .unwrap();

    let leder_token = idp.token(&leder_sub, "Lise Leder");
    let forer_token = idp.token(&forer_sub, "Frida Fører");

    // Without a policy: the bilag posts without attestering (the v1 behavior).
    let fritt = upload(&state, company, &leder_token, "fritt.pdf").await;
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{fritt}/bokfor"),
        &forer_token,
        Some("application/json"),
        Some(post_body(9_000_00)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The policy is switched on with a threshold of 5 000 and Lise as the
    // designated attestant. Only an admin may set it.
    let policy = json!({
        "aktiv": true,
        "belopsgrense_ore": 5_000_00,
        "attestant_person_id": leder,
    })
    .to_string()
    .into_bytes();
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/attestering/policy"),
        &forer_token,
        Some("application/json"),
        Some(policy.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "policy krever admin");
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/attestering/policy"),
        &leder_token,
        Some("application/json"),
        Some(policy),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Below the threshold: straight through, as before.
    let smatt = upload(&state, company, &leder_token, "smatt.pdf").await;
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{smatt}/bokfor"),
        &forer_token,
        Some("application/json"),
        Some(post_body(1_200_00)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "under grensen: {body}");

    // Above the threshold without attestering: refused — and the bilag stays 'ny'.
    let stort = upload(&state, company, &leder_token, "stort.pdf").await;
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{stort}/bokfor"),
        &forer_token,
        Some("application/json"),
        Some(post_body(40_000_00)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("attestering"),
        "{body}"
    );
    let (_, listing) = request(
        &state,
        "GET",
        &format!("/companies/{company}/inbox?status=ny"),
        &forer_token,
        None,
        None,
    )
    .await;
    assert_eq!(listing["documents"].as_array().unwrap().len(), 1);

    // The regnskapsfører is not the designated attestant.
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{stort}/attester"),
        &forer_token,
        Some("application/json"),
        Some(json!({"godkjent": true}).to_string().into_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Lise rejects first — posting is stopped with the reason.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{stort}/attester"),
        &leder_token,
        Some("application/json"),
        Some(
            json!({"godkjent": false, "note": "mangler bestilling"})
                .to_string()
                .into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{stort}/bokfor"),
        &forer_token,
        Some("application/json"),
        Some(post_body(40_000_00)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("mangler bestilling"),
        "{body}"
    );
    // A rejection without a reason is not a decision.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{stort}/attester"),
        &leder_token,
        Some("application/json"),
        Some(json!({"godkjent": false}).to_string().into_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Så godkjenner hun: nyeste beslutning gjelder, sporet beholder begge.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{stort}/attester"),
        &leder_token,
        Some("application/json"),
        Some(
            json!({"godkjent": true, "note": "bestilling ettersendt"})
                .to_string()
                .into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, trail) = request(
        &state,
        "GET",
        &format!("/companies/{company}/inbox/{stort}/attestering"),
        &forer_token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let trail = trail["trail"].as_array().unwrap();
    assert_eq!(trail.len(), 2, "hele sporet står igjen");
    assert_eq!(trail[0]["decision"], "godkjent");
    assert_eq!(trail[1]["decision"], "avvist");

    // Lise attested — so she cannot post it herself (four eyes).
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{stort}/bokfor"),
        &leder_token,
        Some("application/json"),
        Some(post_body(40_000_00)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("fire øyne"),
        "{body}"
    );
    // Regnskapsføreren bokfører.
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{stort}/bokfor"),
        &forer_token,
        Some("application/json"),
        Some(post_body(40_000_00)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Attestering only decides undecided bilag.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/inbox/{stort}/attester"),
        &leder_token,
        Some("application/json"),
        Some(json!({"godkjent": true}).to_string().into_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Innboks-lista bærer attesteringsstatusen for køen i portalen.
    let (_, listing) = request(
        &state,
        "GET",
        &format!("/companies/{company}/inbox"),
        &forer_token,
        None,
        None,
    )
    .await;
    let bokfort = listing["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["filename"] == "stort.pdf")
        .unwrap();
    assert_eq!(bokfort["attestering"], "godkjent");
    assert_eq!(bokfort["attestert_av"], "Lise Leder");

    // ---- Payment list: four eyes on money going out ----
    let (status, payable) = request(
        &state,
        "GET",
        &format!("/companies/{company}/payments/payable"),
        &forer_token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payable}");
    let entry_id = payable["items"][0]["entry_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, run) = request(
        &state,
        "POST",
        &format!("/companies/{company}/payments/runs"),
        &forer_token,
        Some("application/json"),
        Some(
            json!({"items": [{"entry_id": entry_id}], "execution_date": "2026-07-20"})
                .to_string()
                .into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{run}");
    let run_id = run["run_id"].as_str().unwrap().to_string();
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/payments/runs/{run_id}/approve"),
        &forer_token,
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "oppretteren kan ikke godkjenne"
    );
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("fire øyne"),
        "{body}"
    );
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/payments/runs/{run_id}/approve"),
        &leder_token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // ---- Utlegg: the submitter cannot approve their own claim ----
    let (status, expense) = request(
        &state,
        "POST",
        &format!(
            "/companies/{company}/expenses/utlegg?filename=kvittering.pdf\
             &dato=2026-07-12&belop_ore=45000&beskrivelse=Kontorrekvisita"
        ),
        &forer_token,
        Some("application/pdf"),
        Some(b"kvittering: kontorrekvisita".to_vec()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{expense}");
    let expense_id = expense["expense_id"].as_str().unwrap().to_string();
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/expenses/{expense_id}/approve"),
        &forer_token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "selvgodkjenning stoppes");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("fire øyne"),
        "{body}"
    );
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/expenses/{expense_id}/approve"),
        &leder_token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The policy is append-only: the history shows what applied when.
    let (status, policy) = request(
        &state,
        "GET",
        &format!("/companies/{company}/attestering/policy"),
        &forer_token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(policy["policy"]["aktiv"], true);
    assert_eq!(policy["policy"]["belopsgrense_ore"], 5_000_00);
    assert_eq!(policy["history"].as_array().unwrap().len(), 1);
}
