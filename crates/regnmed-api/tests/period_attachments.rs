//! End-to-end periodelåsing and bilagsvedlegg: lock a period, watch both
//! enforcement layers reject postings into it, reopen as admin (audit
//! trail), attach dokumentasjon to a voucher (also in a locked period),
//! and verify content hashes catch tampering. Requires DATABASE_URL
//! (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::NaiveDate;
use common::{TestIdp, test_state, unique_orgnr};
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<Vec<u8>>,
    content_type: Option<&str>,
) -> (StatusCode, Vec<u8>) {
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
    let bytes = axum::body::to_bytes(response.into_body(), 32 * 1024 * 1024)
        .await
        .unwrap();
    (status, bytes.to_vec())
}

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap_or(serde_json::Value::Null)
}

fn sale(date: NaiveDate, ore: i64) -> VoucherDraft {
    VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: "Salg".into(),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: "1920".into(),
                amount: Ore(ore),
                vat_code: None,
                description: None,
                party_no: None,
                avdeling: None,
                prosjekt: None,
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(-ore),
                vat_code: None,
                description: None,
                party_no: None,
                avdeling: None,
                prosjekt: None,
            },
        ],
    }
}

fn date(m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, d).unwrap()
}

#[tokio::test]
async fn period_lock_and_attachments_end_to_end() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    // Admin (direct member) and accountant (bokforing via engagement).
    let admin_sub = format!("test|{}", Uuid::new_v4());
    let accountant_sub = format!("test|{}", Uuid::new_v4());
    let admin = regnmed_db::ensure_person(&state.pool, &admin_sub, Some("Astrid Admin"), None)
        .await
        .unwrap();
    let accountant =
        regnmed_db::ensure_person(&state.pool, &accountant_sub, Some("Kari Bokfører"), None)
            .await
            .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Periodetest AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, admin, "admin")
        .await
        .unwrap();
    let firm = regnmed_db::ensure_firm(&state.pool, &unique_orgnr(), "Tall & Orden AS", "regnskap")
        .await
        .unwrap();
    regnmed_db::ensure_firm_member(&state.pool, firm, accountant, "ansatt")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(&state.pool, firm, company, "regnskap")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [("1920", "Bank"), ("3000", "Salg")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let admin_token = idp.token(&admin_sub, "Astrid Admin");
    let accountant_token = idp.token(&accountant_sub, "Kari Bokfører");

    // January voucher, then the accountant locks through 31 January.
    let january =
        regnmed_db::post_voucher(&state.pool, company, &sale(date(1, 15), 100_00), "test")
            .await
            .unwrap();
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/period-lock"),
        &accountant_token,
        Some(br#"{"locked_through":"2026-01-31"}"#.to_vec()),
        Some("application/json"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Layer 1: posting into the locked period is rejected with a clear error.
    let rejected =
        regnmed_db::post_voucher(&state.pool, company, &sale(date(1, 20), 50_00), "test")
            .await
            .expect_err("locked period must reject postings");
    assert!(
        rejected.to_string().contains("locked through"),
        "got: {rejected}"
    );

    // Layer 2: even a hand-inserted voucher is stopped by the trigger.
    let mut tx = state.pool.begin().await.unwrap();
    let journal_id: Uuid =
        sqlx::query_scalar("select id from journal where company_id = $1 and code = 'GL'")
            .bind(company)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let trigger_err = sqlx::query(
        "insert into voucher (id, company_id, journal_id, fiscal_year, voucher_number,
                              voucher_date, description, created_by, created_at,
                              chain_seq, prev_hash, hash)
         values ($1, $2, $3, 2026, 888888, '2026-01-21', 'smugling', 'test', now(),
                 888888, $4, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(company)
    .bind(journal_id)
    .bind([0u8; 32].as_slice())
    .execute(&mut *tx)
    .await
    .expect_err("database trigger must reject locked-period insert");
    assert!(
        trigger_err.to_string().contains("locked"),
        "got: {trigger_err}"
    );
    drop(tx);

    // February posting is fine.
    let february =
        regnmed_db::post_voucher(&state.pool, company, &sale(date(2, 5), 200_00), "test")
            .await
            .unwrap();

    // Reopening: accountant is refused, admin succeeds — and the history
    // keeps both entries.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/period-lock"),
        &accountant_token,
        Some(br#"{"locked_through":"2026-01-01"}"#.to_vec()),
        Some("application/json"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "reopening needs admin");
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/period-lock"),
        &admin_token,
        Some(br#"{"locked_through":"2026-01-01"}"#.to_vec()),
        Some("application/json"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, lock) = request(
        &state,
        "GET",
        &format!("/companies/{company}/period-lock"),
        &accountant_token,
        None,
        None,
    )
    .await;
    let lock = json(&lock);
    assert_eq!(lock["locked_through"], "2026-01-01");
    assert_eq!(lock["history"].as_array().unwrap().len(), 2, "audit trail");

    // Attachments: upload to the January voucher (its period was locked —
    // completing dokumentasjon is allowed, changing history is not).
    let content = b"%PDF-1.4 kvittering for salg".to_vec();
    let (status, body) = request(
        &state,
        "POST",
        &format!(
            "/companies/{company}/vouchers/{}/attachments?filename=kvittering.pdf",
            january.id
        ),
        &accountant_token,
        Some(content.clone()),
        Some("application/pdf"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", json(&body));
    let uploaded = json(&body);
    let expected_hash = regnmed_core::hash::sha256(&content);
    let expected_hex: String = expected_hash.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(uploaded["sha256"], expected_hex.as_str());
    let attachment_id = uploaded["attachment_id"].as_str().unwrap().to_string();

    // Download round-trips the exact bytes.
    let (status, downloaded) = request(
        &state,
        "GET",
        &format!("/companies/{company}/attachments/{attachment_id}"),
        &accountant_token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(downloaded, content);

    // Append-only: UPDATE and DELETE are rejected by the database.
    let update = sqlx::query("update attachment set filename = 'x' where id = $1")
        .bind(Uuid::parse_str(&attachment_id).unwrap())
        .execute(&state.pool)
        .await
        .expect_err("attachment UPDATE must be rejected");
    assert!(update.to_string().contains("append-only"), "got: {update}");

    // Tampering with content is caught by hash verification.
    assert_eq!(
        regnmed_db::verify_attachments(&state.pool, company)
            .await
            .unwrap(),
        1
    );
    let mut conn = state.pool.acquire().await.unwrap();
    sqlx::query("set session_replication_role = replica")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("update attachment set content = 'forfalsket'::bytea where id = $1")
        .bind(Uuid::parse_str(&attachment_id).unwrap())
        .execute(&mut *conn)
        .await
        .unwrap();
    let tampered = regnmed_db::verify_attachments(&state.pool, company)
        .await
        .expect_err("altered dokumentasjon must fail verification");
    assert!(tampered.to_string().contains("altered"), "got: {tampered}");
    // Restore for a clean dev database.
    sqlx::query("update attachment set content = $2 where id = $1")
        .bind(Uuid::parse_str(&attachment_id).unwrap())
        .bind(&content)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("set session_replication_role = origin")
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);

    // Voucher listing gives the web the ids it needs.
    let (_, vouchers) = request(
        &state,
        "GET",
        &format!("/companies/{company}/vouchers"),
        &accountant_token,
        None,
        None,
    )
    .await;
    let vouchers = json(&vouchers);
    assert_eq!(vouchers["vouchers"].as_array().unwrap().len(), 2);

    // Everything still verifies.
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 2);
    let _ = february;
}
