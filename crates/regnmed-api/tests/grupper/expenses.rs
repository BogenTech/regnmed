//! Utlegg og kjøregodtgjørelse end to end: immutable receipt from
//! submission, one-way decisions (avvisning requires note), approval
//! posting kostnad + mva mot mellomregning with the receipt attached
//! to the voucher, kjøregodtgjørelse from the dated satsregister with
//! the trekkpliktige del surfaced as a warning, and utbetaling posting
//! mellomregning mot bank. Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
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

async fn balance(state: &AppState, company: Uuid, account: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(account)
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
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Utlegg AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
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
    let expense_id = made["expense_id"].as_str().unwrap().to_string();

    // The same receipt again is a double submission, not a second claim.
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "{base}/utlegg?filename=taxi-igjen.txt&dato=2026-07-01&belop_ore=62500&beskrivelse=Taxi"
                ))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "text/plain")
                .body(Body::from(receipt.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let message = String::from_utf8_lossy(&bytes);
    assert!(message.contains("allerede sendt inn"), "{message}");
    // A double tap: the second submission of the same receipt waits on
    // the per-company-and-hash advisory lock, then sees the first claim.
    // Held from outside here, so the wait is observed, not hoped for.
    let parking = b"kvittering: parkering 99,00 kr".to_vec();
    let upload = |name: &str| {
        router(state.clone()).oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "{base}/utlegg?filename={name}&dato=2026-07-02&belop_ore=9900&beskrivelse=Parkering"
                ))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "text/plain")
                .body(Body::from(parking.clone()))
                .unwrap(),
        )
    };
    assert_eq!(upload("p1.txt").await.unwrap().status(), StatusCode::OK);
    let mut held = state.pool.begin().await.unwrap();
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text || encode($2, 'hex'), 0))")
        .bind(company)
        .bind(regnmed_core::hash::sha256(&parking).to_vec())
        .execute(&mut *held)
        .await
        .unwrap();
    let second = tokio::spawn(upload("p2.txt"));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !second.is_finished(),
        "the second submission must wait for the lock"
    );
    held.rollback().await.unwrap();
    assert_eq!(
        second.await.unwrap().unwrap().status(),
        StatusCode::BAD_REQUEST
    );

    // A missing amount is a sentence, not a deserializer error.
    let (status, body) = request(
        &state,
        "POST",
        &format!("{base}/kjoring"),
        &token,
        Some(serde_json::json!({ "dato": "2026-07-01", "beskrivelse": "Tur" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.to_string().contains("oppgi antall kilometer"),
        "{body}"
    );

    // Receipt round-trips integrity-checked; content is DB-immutable.
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("{base}/{expense_id}/receipt"))
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
        .bind(Uuid::parse_str(&expense_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "receipt is immutable from submission");
    let tamper = sqlx::query("delete from expense where id = $1")
        .bind(Uuid::parse_str(&expense_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "claims are never deleted");

    // Approve with inngående mva (code 1, 25 %): 625 = 500 + 125.
    let (status, approved) = request(
        &state,
        "POST",
        &format!("{base}/{expense_id}/approve"),
        &token,
        Some(serde_json::json!({ "mva_kode": "1" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {approved}");
    assert!(approved["warning"].is_null());
    assert_eq!(balance(&state, company, "7790").await, 500_00);
    assert_eq!(balance(&state, company, "2710").await, 125_00);
    assert_eq!(balance(&state, company, "2910").await, -625_00);
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
        &format!("{base}/{expense_id}/approve"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "decisions are one-way");

    // The form asks for the satser first, so it can show the amount.
    let (status, rate) = request(
        &state,
        "GET",
        &format!("{base}/kjoring?dato=2026-03-10"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rate}");
    assert_eq!(rate["sats_ore_per_km"], 530);
    assert_eq!(rate["trekkfri_ore_per_km"], 350);

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
    let run_id = made["expense_id"].as_str().unwrap().to_string();
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
        &format!("{base}/{run_id}/approve"),
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
    assert_eq!(balance(&state, company, "7100").await, 636_00);

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
        &format!("{base}/{expense_id}/pay"),
        &token,
        Some(serde_json::json!({ "dato": "2026-07-05" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {paid}");
    assert_eq!(
        balance(&state, company, "2910").await,
        -636_00,
        "kjøringen står igjen"
    );
    assert_eq!(balance(&state, company, "1920").await, -625_00);
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{expense_id}/pay"),
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
    assert_eq!(statuses.len(), 4);
    assert!(statuses.contains(&"utbetalt".into()));
    assert!(statuses.contains(&"godkjent".into()));
    assert!(statuses.contains(&"avvist".into()));
    assert_eq!(listed["total"], 4);
    assert_eq!(listed["counts"]["utbetalt"], 1);
    assert_eq!(listed["counts"]["godkjent"], 1);
    assert_eq!(listed["counts"]["avvist"], 1);
    assert_eq!(listed["counts"]["innsendt"], 1, "the parking claim");
    // The tabs: «åpne» is what still owes money, the rest are done.
    let (_, open) = request(
        &state,
        "GET",
        &format!("{base}?status=apne&limit=1&offset=5"),
        &token,
        None,
    )
    .await;
    assert_eq!(
        open["total"], 2,
        "a page past the end still knows the total"
    );
    assert!(open["expenses"].as_array().unwrap().is_empty());
    let (_, open) = request(&state, "GET", &format!("{base}?status=apne"), &token, None).await;
    let open_ids: Vec<&str> = open["expenses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["expense_id"].as_str().unwrap())
        .collect();
    assert!(open_ids.contains(&run_id.as_str()), "{open}");
    let (status, _) = request(&state, "GET", &format!("{base}?status=tull"), &token, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // One claim, with both vouchers it produced.
    let (status, one) = request(&state, "GET", &format!("{base}/{expense_id}"), &token, None).await;
    assert_eq!(status, StatusCode::OK, "{one}");
    assert_eq!(one["status"], "utbetalt");
    assert!(
        one["voucher"].is_string() && one["utbetalt_voucher"].is_string(),
        "{one}"
    );
    assert_ne!(one["voucher"], one["utbetalt_voucher"]);
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 3, "utlegg + kjøring + utbetaling");
}
