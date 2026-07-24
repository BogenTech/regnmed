//! Integration tests of the ledger's guarantees against a real Postgres:
//! gap-free numbering, the append-only triggers, the deferred balance
//! check, tamper detection by chain verification, and the SAF-T loader's
//! period arithmetic. Requires DATABASE_URL (skips otherwise) —
//! `scripts/dev-db.sh` + `regnmed migrate` provides it locally; CI runs a
//! postgres:18 service.

use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> Option<PgPool> {
    dotenvy::dotenv().ok();
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL not set");
        return None;
    };
    let pool = regnmed_db::connect(&url).await.expect("connect to dev db");
    regnmed_db::MIGRATOR.run(&pool).await.expect("migrate");
    Some(pool)
}

fn unique_orgnr() -> String {
    let n = u32::from_be_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap());
    format!("{:09}", u64::from(n) % 1_000_000_000)
}

/// Fresh company with a journal and a few NS 4102 accounts, isolated from
/// everything else in the shared dev database.
async fn seeded_company(pool: &PgPool) -> Uuid {
    let company = regnmed_db::create_company(pool, &unique_orgnr(), "Testselskap AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1920", "Bankinnskudd"),
        ("3000", "Salgsinntekt, avgiftspliktig"),
        ("2700", "Utgående merverdiavgift"),
    ] {
        regnmed_db::ensure_account(pool, company, number, name)
            .await
            .unwrap();
    }
    company
}

/// A balanced two-line voucher: bank debit against sales credit.
fn sale(date: NaiveDate, ore: i64, vat_code: Option<&str>) -> VoucherDraft {
    VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: "Testsalg".into(),
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
                vat_code: vat_code.map(str::to_owned),
                description: Some("Salg".into()),
                party_no: None,
                avdeling: None,
                prosjekt: None,
            },
        ],
    }
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

#[tokio::test]
async fn voucher_numbers_stay_gap_free_across_failed_postings() {
    let Some(pool) = pool().await else { return };
    let company = seeded_company(&pool).await;

    let first = regnmed_db::post_voucher(
        &pool,
        company,
        &sale(date(2026, 1, 10), 100_00, None),
        "test",
    )
    .await
    .unwrap();
    assert_eq!(first.voucher_number, 1);

    // Fails inside the posting transaction (unknown account), after the
    // counter would have been bumped — the rollback must roll the number
    // back too, or the sequence gaps.
    let mut bad = sale(date(2026, 1, 11), 100_00, None);
    bad.entries[0].account_number = "9999".into();
    regnmed_db::post_voucher(&pool, company, &bad, "test")
        .await
        .expect_err("unknown account must fail the posting");

    let second = regnmed_db::post_voucher(
        &pool,
        company,
        &sale(date(2026, 1, 12), 200_00, None),
        "test",
    )
    .await
    .unwrap();
    assert_eq!(
        second.voucher_number, 2,
        "failed posting must not burn a number"
    );
    assert_eq!(second.chain_seq, first.chain_seq + 1);
}

#[tokio::test]
async fn ledger_rows_reject_update_and_delete() {
    let Some(pool) = pool().await else { return };
    let company = seeded_company(&pool).await;
    let posted = regnmed_db::post_voucher(
        &pool,
        company,
        &sale(date(2026, 2, 1), 500_00, None),
        "test",
    )
    .await
    .unwrap();

    let update = sqlx::query("update voucher set description = 'omskrevet' where id = $1")
        .bind(posted.id)
        .execute(&pool)
        .await
        .expect_err("UPDATE on voucher must be rejected");
    assert!(update.to_string().contains("append-only"), "got: {update}");

    let delete = sqlx::query("delete from entry where voucher_id = $1")
        .bind(posted.id)
        .execute(&pool)
        .await
        .expect_err("DELETE on entry must be rejected");
    assert!(delete.to_string().contains("append-only"), "got: {delete}");
}

#[tokio::test]
async fn database_rechecks_double_entry_balance_at_commit() {
    let Some(pool) = pool().await else { return };
    let company = seeded_company(&pool).await;

    // Bypass the domain layer entirely: hand-insert a voucher with a single
    // unbalanced line. The deferred constraint trigger must reject the
    // commit — this is the database's independent second layer.
    let mut tx = pool.begin().await.unwrap();
    let voucher_id = Uuid::now_v7();
    let journal_id: Uuid =
        sqlx::query_scalar("select id from journal where company_id = $1 and code = 'GL'")
            .bind(company)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let account_id: Uuid =
        sqlx::query_scalar("select id from account where company_id = $1 and number = '1920'")
            .bind(company)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query(
        "insert into voucher (id, company_id, journal_id, fiscal_year, voucher_number,
                              voucher_date, description, created_by, created_at,
                              chain_seq, prev_hash, hash)
         values ($1, $2, $3, 2026, 999999, '2026-02-02', 'ubalansert', 'test', now(),
                 999999, $4, $4)",
    )
    .bind(voucher_id)
    .bind(company)
    .bind(journal_id)
    .bind([0u8; 32].as_slice())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "insert into entry (id, voucher_id, line_no, account_id, amount_ore)
         values ($1, $2, 1, $3, 10000)",
    )
    .bind(Uuid::now_v7())
    .bind(voucher_id)
    .bind(account_id)
    .execute(&mut *tx)
    .await
    .unwrap();

    let err = tx
        .commit()
        .await
        .expect_err("unbalanced voucher must not commit");
    let message = err.to_string();
    assert!(
        message.contains("at least two") || message.contains("balance"),
        "got: {message}"
    );
}

#[tokio::test]
async fn verify_chain_detects_and_survives_repair_of_tampering() {
    let Some(pool) = pool().await else { return };
    let company = seeded_company(&pool).await;
    let first = regnmed_db::post_voucher(
        &pool,
        company,
        &sale(date(2026, 3, 1), 300_00, None),
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        &pool,
        company,
        &sale(date(2026, 3, 2), 400_00, None),
        "test",
    )
    .await
    .unwrap();

    let report = regnmed_db::verify_chain(&pool, company).await.unwrap();
    assert_eq!(report.vouchers_checked, 2);

    // Simulate a DBA-level adversary: superuser disables triggers for the
    // session (session_replication_role = replica) and edits history.
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("set session_replication_role = replica")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "update entry set amount_ore = amount_ore + 100 where voucher_id = $1 and line_no = 1",
    )
    .bind(first.id)
    .execute(&mut *conn)
    .await
    .unwrap();

    let err = regnmed_db::verify_chain(&pool, company)
        .await
        .expect_err("tampered chain must fail verification");
    assert!(err.to_string().contains("tampered"), "got: {err}");

    // Restore the original amount (leaves the dev database clean) and the
    // chain verifies again.
    sqlx::query(
        "update entry set amount_ore = amount_ore - 100 where voucher_id = $1 and line_no = 1",
    )
    .bind(first.id)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("set session_replication_role = origin")
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);

    let report = regnmed_db::verify_chain(&pool, company).await.unwrap();
    assert_eq!(report.vouchers_checked, 2);
}

#[tokio::test]
async fn saft_input_splits_balances_at_the_period_boundary() {
    let Some(pool) = pool().await else { return };
    let company = seeded_company(&pool).await;

    // One voucher before the period, one inside it.
    regnmed_db::post_voucher(
        &pool,
        company,
        &sale(date(2026, 1, 15), 100_00, None),
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        &pool,
        company,
        &sale(date(2026, 3, 10), 50_00, Some("3")),
        "test",
    )
    .await
    .unwrap();

    let input = regnmed_db::load_saft_input(
        &pool,
        company,
        date(2026, 3, 1),
        date(2026, 12, 31),
        "Kari",
        "Nordmann",
    )
    .await
    .unwrap();

    let bank = input.accounts.iter().find(|a| a.number == "1920").unwrap();
    assert_eq!(
        bank.opening_ore, 100_00,
        "January posting is opening balance"
    );
    assert_eq!(
        bank.closing_ore, 150_00,
        "both postings are in closing balance"
    );

    let transactions: Vec<_> = input
        .journals
        .iter()
        .flat_map(|j| &j.transactions)
        .collect();
    assert_eq!(
        transactions.len(),
        1,
        "only the in-period voucher is exported"
    );
    assert_eq!(transactions[0].number, 2);
    assert_eq!(transactions[0].lines.len(), 2);

    assert!(
        input
            .tax_codes
            .iter()
            .any(|t| t.code == "3" && t.percent_bp == 2500),
        "vat code used in the period is in the tax table"
    );

    // The rendered file for this input must satisfy the official schema.
    let xml = regnmed_core::saft::render(&input).unwrap();
    assert!(xml.contains("<OpeningDebitBalance>100.00</OpeningDebitBalance>"));
    assert!(xml.contains("<ClosingDebitBalance>150.00</ClosingDebitBalance>"));
}
