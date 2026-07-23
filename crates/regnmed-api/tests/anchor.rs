//! External anchoring end to end: a snapshot freezes two companies'
//! chain heads under one Merkle root; the public feed exposes only
//! roots; per-company anchors carry inclusion proofs that verify
//! independently with regnmed-core; the verify endpoint reports a clean
//! chain — and detects a mismatching anchor when one is planted.
//! Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use regnmed_core::Ore;
use regnmed_core::anchor::{AnchorLeaf, InclusionProof, ProofStep, Side, verify_inclusion};
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn get(state: &AppState, uri: &str, bearer: Option<&str>) -> (StatusCode, serde_json::Value) {
    let mut request = Request::builder().method("GET").uri(uri);
    if let Some(token) = bearer {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = router(state.clone())
        .oneshot(request.body(Body::empty()).unwrap())
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

async fn post_simple_voucher(pool: &sqlx::PgPool, company: Uuid, ore: i64) {
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: chrono::Utc::now().date_naive(),
        description: "Salg".into(),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: "1920".into(),
                amount: Ore(ore),
                vat_code: None,
                description: None,
                party_no: None,
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(-ore),
                vat_code: None,
                description: None,
                party_no: None,
            },
        ],
    };
    regnmed_db::post_voucher(pool, company, &draft, "test")
        .await
        .unwrap();
}

fn hash32(hex: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap();
    }
    out
}

#[tokio::test]
async fn anchors_publish_roots_and_proofs_and_detect_rewrites() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Randi Revisor"), None)
        .await
        .unwrap();
    let token = idp.token(&sub, "Randi Revisor");

    // Two companies with history, so the Merkle tree has real siblings.
    let mut company_ids = Vec::new();
    for _ in 0..2 {
        let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Forankret AS")
            .await
            .unwrap();
        regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
            .await
            .unwrap();
        regnmed_db::ensure_account(&state.pool, company, "1920", "Bank")
            .await
            .unwrap();
        regnmed_db::ensure_account(&state.pool, company, "3000", "Salg")
            .await
            .unwrap();
        post_simple_voucher(&state.pool, company, 100_00).await;
        post_simple_voucher(&state.pool, company, 250_00).await;
        company_ids.push(company);
    }
    let company = company_ids[0];
    regnmed_db::ensure_company_member(&state.pool, company, person, "les")
        .await
        .unwrap();

    let snapshot = regnmed_db::create_anchor_snapshot(&state.pool)
        .await
        .unwrap()
        .expect("companies with vouchers exist");
    assert!(snapshot.leaf_count >= 2);

    // The transparency feed is public — no token — and carries the root.
    let (status, feed) = get(&state, "/anchors", None).await;
    assert_eq!(status, StatusCode::OK);
    let published = feed["snapshots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["snapshot_id"] == snapshot.id.to_string())
        .expect("new snapshot in the public feed");
    assert_eq!(published["root_hash"], hex::encode(snapshot.root_hash));

    // Per-company anchors need access: strangers get 404, members get
    // the anchored head plus an inclusion proof that verifies offline
    // against the public root using only regnmed-core.
    let (status, _) = get(
        &state,
        &format!("/companies/{}/anchors", company_ids[1]),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, anchors) = get(
        &state,
        &format!("/companies/{company}/anchors"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let anchor = &anchors["anchors"][0];
    assert_eq!(anchor["last_seq"], 2);
    let leaf = AnchorLeaf {
        company_id: company,
        last_seq: anchor["last_seq"].as_i64().unwrap(),
        last_hash: hash32(anchor["last_hash"].as_str().unwrap()),
    };
    let proof: InclusionProof = anchor["proof"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| ProofStep {
            side: if step["side"] == "left" {
                Side::Left
            } else {
                Side::Right
            },
            sibling: hash32(step["sibling"].as_str().unwrap()),
        })
        .collect();
    let root = hash32(published["root_hash"].as_str().unwrap());
    assert!(
        verify_inclusion(&leaf, &proof, &root),
        "the API's proof must connect the company's head to the public root"
    );

    // Read access suffices to verify; the untouched chain checks out.
    let (status, check) = get(
        &state,
        &format!("/companies/{company}/anchors/verify"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(check["ok"], true, "problems: {}", check["problems"]);
    assert_eq!(check["vouchers_checked"], 2);
    assert_eq!(check["anchors_checked"], 1);

    // Plant an anchor claiming a different head hash — as if history had
    // been rewritten after anchoring. Verification must name the rewrite.
    let fake_snapshot = Uuid::now_v7();
    sqlx::query("insert into anchor_snapshot (id, root_hash, leaf_count) values ($1, $2, 1)")
        .bind(fake_snapshot)
        .bind([0xEE_u8; 32].as_slice())
        .execute(&state.pool)
        .await
        .unwrap();
    sqlx::query(
        "insert into anchor_leaf (snapshot_id, company_id, last_seq, last_hash)
         values ($1, $2, 2, $3)",
    )
    .bind(fake_snapshot)
    .bind(company)
    .bind([0xEE_u8; 32].as_slice())
    .execute(&state.pool)
    .await
    .unwrap();
    let (_, check) = get(
        &state,
        &format!("/companies/{company}/anchors/verify"),
        Some(&token),
    )
    .await;
    assert_eq!(check["ok"], false);
    let problems = check["problems"].to_string();
    assert!(
        problems.contains("rewritten") && problems.contains("does not recompute"),
        "both the head mismatch and the bogus root are reported: {problems}"
    );

    // The anchor evidence itself is append-only, enforced by the database.
    let denied = sqlx::query("update anchor_snapshot set leaf_count = 99 where id = $1")
        .bind(snapshot.id)
        .execute(&state.pool)
        .await;
    assert!(denied.is_err(), "anchor rows must reject UPDATE");
    let denied = sqlx::query("delete from anchor_leaf where snapshot_id = $1")
        .bind(fake_snapshot)
        .execute(&state.pool)
        .await;
    assert!(denied.is_err(), "anchor rows must reject DELETE");
}
