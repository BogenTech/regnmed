//! Utlegg og kjøregodtgjørelse end to end: immutable receipt from
//! submission, one-way decisions (avvisning requires note), approval
//! posting kostnad + mva mot mellomregning with the receipt attached
//! to the voucher, kjøregodtgjørelse from the dated satsregister with
//! the trekkpliktige del surfaced as a warning, and utbetaling posting
//! mellomregning mot bank. Requires DATABASE_URL (skips otherwise).

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

async fn saldo(state: &AppState, company: Uuid, konto: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(konto)
    .fetch_one(&state.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn expenses_flow() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Reisende Medarbeider"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Utlegg AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("7790", "Annen kostnad"),
        ("7100", "Bilgodtgjørelse"),
        ("2710", "Inngående mva"),
        ("2910", "Gjeld til ansatte"),
        ("1920", "Bank"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let token = idp.token(&sub, "Reisende Medarbeider");
    let base = format!("/companies/{company}/expenses");

    // Utlegg with a receipt, uploaded as a raw body.
    let receipt = b"kvittering: taxi 625,00 kr inkl. mva".to_vec();
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "{base}/utlegg?filename=taxi.txt&dato=2026-07-01&belop_ore=62500&beskrivelse=Taxi%20kundebes%C3%B8k"
                ))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "text/plain")
                .body(Body::from(receipt.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let made: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let utlegg_id = made["expense_id"].as_str().unwrap().to_string();

    // Receipt round-trips integrity-checked; content is DB-immutable.
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("{base}/{utlegg_id}/receipt"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(bytes.to_vec(), receipt);
    let tamper = sqlx::query("update expense set receipt_content = 'forfalsket' where id = $1")
        .bind(Uuid::parse_str(&utlegg_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "receipt is immutable from submission");
    let tamper = sqlx::query("delete from expense where id = $1")
        .bind(Uuid::parse_str(&utlegg_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "claims are never deleted");

    // Approve with inngående mva (code 1, 25 %): 625 = 500 + 125.
    let (status, approved) = request(
        &state,
        "POST",
        &format!("{base}/{utlegg_id}/approve"),
        &token,
        Some(serde_json::json!({ "mva_kode": "1" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {approved}");
    assert!(approved["warning"].is_null());
    assert_eq!(saldo(&state, company, "7790").await, 500_00);
    assert_eq!(saldo(&state, company, "2710").await, 125_00);
    assert_eq!(saldo(&state, company, "2910").await, -625_00);
    // The receipt followed the claim onto the voucher (oppbevaring).
    let attachment_count: i64 = sqlx::query_scalar(
        "select count(*) from attachment a
         join voucher v on v.id = a.voucher_id
         where v.company_id = $1 and a.filename = 'taxi.txt'",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(attachment_count, 1);
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{utlegg_id}/approve"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "decisions are one-way");

    // Kjøregodtgjørelse: 120 km on 2026-03-10 → 5,30/3,50 kr satser.
    let (status, made) = request(
        &state,
        "POST",
        &format!("{base}/kjoring"),
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-03-10", "beskrivelse": "Oslo–Drammen t/r, kundemøte", "km": 120,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {made}");
    assert_eq!(made["belop_ore"], 63_600);
    assert_eq!(made["trekkpliktig_ore"], 21_600);
    let kjoring_id = made["expense_id"].as_str().unwrap().to_string();
    // Before the register's coverage: refused loudly, never guessed.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/kjoring"),
        &token,
        Some(
            serde_json::json!({ "dato": "2024-03-10", "beskrivelse": "Gammel tur", "km": 10 })
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Approval carries the honest trekkpliktig warning.
    let (status, approved) = request(
        &state,
        "POST",
        &format!("{base}/{kjoring_id}/approve"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        approved["warning"]
            .as_str()
            .unwrap()
            .contains("trekkpliktig del 216,00 kr"),
        "warning: {approved}"
    );
    assert_eq!(saldo(&state, company, "7100").await, 636_00);

    // Avvisning requires a note; the note sticks.
    let (status, made) = request(
        &state,
        "POST",
        &format!("{base}/kjoring"),
        &token,
        Some(
            serde_json::json!({ "dato": "2026-07-02", "beskrivelse": "Privat tur", "km": 30 })
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let privat_id = made["expense_id"].as_str().unwrap().to_string();
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{privat_id}/reject"),
        &token,
        Some(serde_json::json!({ "note": "  " }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "tom begrunnelse avvises");
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{privat_id}/reject"),
        &token,
        Some(serde_json::json!({ "note": "privat kjøring dekkes ikke" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Illegal transition straight in the database: trigger says no.
    let tamper = sqlx::query("update expense set status = 'utbetalt' where id = $1")
        .bind(Uuid::parse_str(&privat_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "avvist → utbetalt is not a transition");

    // Utbetaling: mellomregning → bank, one-way; pay of innsendt fails.
    let (status, paid) = request(
        &state,
        "POST",
        &format!("{base}/{utlegg_id}/pay"),
        &token,
        Some(serde_json::json!({ "dato": "2026-07-05" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {paid}");
    assert_eq!(
        saldo(&state, company, "2910").await,
        -636_00,
        "kjøringen står igjen"
    );
    assert_eq!(saldo(&state, company, "1920").await, -625_00);
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{utlegg_id}/pay"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "already paid");

    // The list tells the whole story, and the chain verifies.
    let (_, listed) = request(&state, "GET", &base, &token, None).await;
    let statuses: Vec<String> = listed["expenses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["status"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(statuses.len(), 3);
    assert!(statuses.contains(&"utbetalt".into()));
    assert!(statuses.contains(&"godkjent".into()));
    assert!(statuses.contains(&"avvist".into()));
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 3, "utlegg + kjøring + utbetaling");
}
