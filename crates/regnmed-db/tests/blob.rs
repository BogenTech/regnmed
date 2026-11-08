//! Documents in the object store, end to end (docs/blob-store.md).
//! Runs only with BOTH DATABASE_URL and BLOB_S3_* set — against a fresh
//! database, because `offload_documents` moves every document in it.

use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use regnmed_db::blob::{Blobs, blobs, offload_documents};
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> Option<PgPool> {
    dotenvy::dotenv().ok();
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL not set");
        return None;
    };
    if !matches!(blobs(), Blobs::S3(_)) {
        eprintln!("skipping: BLOB_S3_* not set");
        return None;
    }
    let pool = regnmed_db::connect(&url).await.expect("connect");
    regnmed_db::MIGRATOR.run(&pool).await.expect("migrate");
    Some(pool)
}

#[tokio::test]
async fn documents_live_in_the_object_store_and_old_rows_move_there() {
    let Some(pool) = pool().await else { return };
    let n = u32::from_be_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap());
    let company =
        regnmed_db::create_company(&pool, &format!("{:09}", n % 1_000_000_000), "Blob AS")
            .await
            .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (no, name) in [("1920", "Bank"), ("3000", "Salg")] {
        regnmed_db::ensure_account(&pool, company, no, name)
            .await
            .unwrap();
    }
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        description: "blob".into(),
        reverses: None,
        entries: [("1920", 100_00), ("3000", -100_00)]
            .map(|(account, ore)| EntryDraft {
                account_number: account.into(),
                amount: Ore(ore),
                vat_code: None,
                description: None,
                party_no: None,
                department: None,
                project: None,
                currency: None,
            })
            .to_vec(),
    };
    let voucher = regnmed_db::post_voucher(&pool, company, &draft, "test")
        .await
        .unwrap();

    // A new document goes to the store; the row keeps only its hash.
    let bytes = format!("%PDF blob {}", Uuid::new_v4()).into_bytes();
    let meta = regnmed_db::add_attachment(
        &pool,
        company,
        voucher.id,
        "a.pdf",
        "application/pdf",
        &bytes,
        "test",
    )
    .await
    .unwrap();
    let in_row: Option<Vec<u8>> =
        sqlx::query_scalar("select content from attachment where id = $1")
            .bind(meta.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(in_row, None, "a new document must not be stored in the row");
    let (_, back) = regnmed_db::get_attachment(&pool, company, meta.id)
        .await
        .unwrap();
    assert_eq!(back, bytes);

    // A document from before the store (bytes in the row) moves out.
    let old = format!("%PDF old {}", Uuid::new_v4()).into_bytes();
    let old_id = Uuid::now_v7();
    sqlx::query(
        "insert into attachment (id, company_id, voucher_id, filename, content_type,
                                 byte_size, sha256, content, uploaded_by)
         values ($1, $2, $3, 'old.pdf', 'application/pdf', $4, sha256($5), $5, 'test')",
    )
    .bind(old_id)
    .bind(company)
    .bind(voucher.id)
    .bind(old.len() as i64)
    .bind(&old)
    .execute(&pool)
    .await
    .unwrap();
    let moved = offload_documents(&pool).await.unwrap();
    assert!(
        moved.iter().any(|(t, c)| *t == "attachment" && *c >= 1),
        "{moved:?}"
    );
    let (_, old_back) = regnmed_db::get_attachment(&pool, company, old_id)
        .await
        .unwrap();
    assert_eq!(old_back, old);
    assert_eq!(
        regnmed_db::verify_attachments(&pool, company)
            .await
            .unwrap(),
        2
    );
}
