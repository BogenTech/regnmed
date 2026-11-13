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

fn unique_orgnumber() -> String {
    let n = u32::from_be_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap());
    format!("{:09}", u64::from(n) % 1_000_000_000)
}

/// Fresh company with a journal and a few NS 4102 accounts, isolated from
/// everything else in the shared dev database.
async fn seeded_company(pool: &PgPool) -> Uuid {
    let company = regnmed_db::create_company(pool, &unique_orgnumber(), "Testselskap AS")
        .await
        .unwrap();
    // A VAT-registered seller: VAT codes need a registration on the
    // voucher's date (post_voucher_in, docs/mva.md).
    regnmed_db::record_registrering(
        pool,
        company,
        chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
        regnmed_db::Registrering {
            vat_registered: true,
            business_registered: true,
        },
        "manuell",
        Some("testfixtur"),
        "test",
    )
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
                department: None,
                project: None,
                currency: None,
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(-ore),
                vat_code: vat_code.map(str::to_owned),
                description: Some("Salg".into()),
                party_no: None,
                department: None,
                project: None,
                currency: None,
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

/// ensure_person runs on every authenticated request, so an unchanged
/// person must not be written: a row's xmin moves only when it is updated.
#[tokio::test]
async fn ensure_person_does_not_write_an_unchanged_person() {
    let Some(pool) = pool().await else { return };
    let sub = format!("test-{}", Uuid::new_v4());
    let xmin = |pool: PgPool, sub: String| async move {
        sqlx::query_scalar::<_, String>("select xmin::text from person where oidc_sub = $1")
            .bind(sub)
            .fetch_one(&pool)
            .await
            .unwrap()
    };

    let id = regnmed_db::ensure_person(&pool, &sub, Some("Kari"), Some("kari@example.no"))
        .await
        .unwrap();
    let before = xmin(pool.clone(), sub.clone()).await;

    let again = regnmed_db::ensure_person(&pool, &sub, Some("Kari"), Some("kari@example.no"))
        .await
        .unwrap();
    let no_claims = regnmed_db::ensure_person(&pool, &sub, None, None)
        .await
        .unwrap();
    assert_eq!((again, no_claims), (id, id));
    assert_eq!(
        xmin(pool.clone(), sub.clone()).await,
        before,
        "unchanged person was written"
    );

    // A changed name is still recorded.
    regnmed_db::ensure_person(&pool, &sub, Some("Kari Nordmann"), None)
        .await
        .unwrap();
    assert_ne!(xmin(pool.clone(), sub.clone()).await, before);
    let name: String = sqlx::query_scalar("select name from person where oidc_sub = $1")
        .bind(&sub)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "Kari Nordmann");
}

/// Migration 0070: the only update an attachment row allows is emptying
/// its content, and only when the stored hash is the hash of the bytes
/// removed. Afterwards the bytes must come from the object store.
#[tokio::test]
async fn an_attachment_row_allows_only_a_verified_offload() {
    let Some(pool) = pool().await else { return };
    let company = seeded_company(&pool).await;
    let voucher =
        regnmed_db::post_voucher(&pool, company, &sale(date(2026, 2, 1), 50_00, None), "test")
            .await
            .unwrap();
    let meta = regnmed_db::add_attachment(
        &pool,
        company,
        voucher.id,
        "k.pdf",
        "application/pdf",
        b"%PDF-kvittering",
        "test",
    )
    .await
    .unwrap();

    let refused = |sql: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query(sql).bind(meta.id).execute(&pool).await.is_err() }
    };
    assert!(refused("update attachment set filename = 'x.pdf' where id = $1").await);
    assert!(
        refused("update attachment set content = null, sha256 = sha256('x'::bytea) where id = $1")
            .await
    );
    assert!(refused("update attachment set content = 'other'::bytea where id = $1").await);
    assert!(refused("delete from attachment where id = $1").await);

    sqlx::query("update attachment set content = null where id = $1")
        .bind(meta.id)
        .execute(&pool)
        .await
        .expect("a verified offload is allowed");
    assert!(
        refused("update attachment set content = '%PDF-kvittering'::bytea where id = $1").await,
        "content cannot come back"
    );
    // This test process has no object store, so the bytes are now
    // unreachable — and that is reported, never served as empty.
    let err = regnmed_db::get_attachment(&pool, company, meta.id)
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("object store"), "{err:#}");
}

/// Debit and credit per account over `[from, to]`, summed straight from the
/// entries: what the monthly totals (migration 0072) must reproduce.
async fn sums_from_entries(
    pool: &PgPool,
    company: Uuid,
    from: Option<NaiveDate>,
    to: NaiveDate,
) -> Vec<(String, i64, i64)> {
    sqlx::query_as(
        "select a.number,
                coalesce(sum(e.amount_ore) filter (where e.amount_ore > 0), 0)::bigint,
                coalesce(sum(e.amount_ore) filter (where e.amount_ore < 0), 0)::bigint
         from entry e join voucher v on v.id = e.voucher_id join account a on a.id = e.account_id
         where v.company_id = $1 and ($2::date is null or v.voucher_date >= $2)
           and v.voucher_date <= $3
         group by a.number order by a.number",
    )
    .bind(company)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn reports_read_from_the_monthly_totals_match_the_entries_exactly() {
    let Some(pool) = pool().await else { return };
    let company = seeded_company(&pool).await;

    // Two years of postings on scattered days, refunds included, posted out
    // of date order (backdating is allowed) -- deterministic, no rand dep.
    let mut seed: u64 = 0x2026_1109;
    let mut next = |m: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % m
    };
    let mut days = Vec::new();
    for _ in 0..160 {
        let day = date(2025, 1, 1) + chrono::Days::new(next(730));
        let ore = (next(500_000) as i64 + 1) * if next(5) == 0 { -1 } else { 1 };
        regnmed_db::post_voucher(&pool, company, &sale(day, ore, None), "test")
            .await
            .unwrap();
        days.push(day);
    }
    // The edges themselves: a month's first and last day.
    for day in [date(2025, 4, 1), date(2025, 4, 30), date(2025, 1, 31)] {
        regnmed_db::post_voucher(&pool, company, &sale(day, 1234, None), "test")
            .await
            .unwrap();
        days.push(day);
    }
    // A posting that fails rolls its totals back with it.
    let mut bad = sale(date(2025, 6, 15), 777, None);
    bad.entries[1].account_number = "9999".into();
    regnmed_db::post_voucher(&pool, company, &bad, "test")
        .await
        .expect_err("unknown account");

    assert!(
        regnmed_db::verify_month_totals(&pool, company)
            .await
            .unwrap()
            .is_empty()
    );

    let mut ranges: Vec<(Option<NaiveDate>, NaiveDate)> = vec![
        (None, date(2026, 12, 31)),
        (None, date(2025, 6, 14)),
        (Some(date(2025, 1, 1)), date(2025, 12, 31)),
        (Some(date(2025, 3, 1)), date(2025, 3, 31)),
        (Some(date(2025, 3, 2)), date(2025, 3, 30)),
        (Some(date(2025, 2, 28)), date(2025, 3, 1)),
        (Some(date(2026, 7, 4)), date(2026, 7, 4)),
        (Some(date(2025, 11, 17)), date(2026, 2, 3)),
        (Some(date(2025, 1, 1)), date(2025, 4, 1)),
        (None, date(2025, 4, 1)),
        (Some(date(2025, 1, 31)), date(2025, 4, 30)),
        (Some(date(2025, 4, 30)), date(2025, 6, 30)),
    ];
    for _ in 0..40 {
        let a = date(2024, 12, 1) + chrono::Days::new(next(800));
        let b = a + chrono::Days::new(next(400));
        ranges.push((if next(4) == 0 { None } else { Some(a) }, b));
    }
    ranges.extend(days.iter().map(|d| (Some(*d), *d)));
    for (from, to) in ranges {
        let read: Vec<(String, i64, i64)> = regnmed_db::account_sums(&pool, company, from, to)
            .await
            .unwrap()
            .into_iter()
            .map(|a| (a.number, a.debit_ore, a.credit_ore))
            .collect();
        assert_eq!(
            read,
            sums_from_entries(&pool, company, from, to).await,
            "{from:?}..{to}"
        );
    }

    // The trial balance built on them closes on the entries' own sum.
    let tb = regnmed_db::trial_balance(&pool, company, date(2026, 3, 10), date(2026, 8, 20))
        .await
        .unwrap();
    let closing = sums_from_entries(&pool, company, None, date(2026, 8, 20)).await;
    let opening = sums_from_entries(&pool, company, None, date(2026, 3, 9)).await;
    for row in &tb {
        let c = closing
            .iter()
            .find(|c| c.0 == row.number)
            .map_or(0, |c| c.1 + c.2);
        let o = opening
            .iter()
            .find(|o| o.0 == row.number)
            .map_or(0, |o| o.1 + o.2);
        assert_eq!((row.opening_ore, row.closing_ore), (o, c), "{}", row.number);
    }
    assert_eq!(tb.len(), closing.len());

    // A total that drifts from its entries (only the owner role can write
    // one) is named by the check verify-ledger runs.
    sqlx::query(
        "update account_month_total t set debit_ore = debit_ore + 1
         from account a where a.id = t.account_id
           and t.company_id = $1 and a.number = '1920' and t.month = '2025-04-01'",
    )
    .bind(company)
    .execute(&pool)
    .await
    .unwrap();
    let problems = regnmed_db::verify_month_totals(&pool, company)
        .await
        .unwrap();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("account 1920 2025-04"),
        "{problems:?}"
    );
    // Put it back: a full `verify-ledger` over the shared dev database
    // must stay green.
    sqlx::query(
        "update account_month_total t set debit_ore = debit_ore - 1
         from account a where a.id = t.account_id
           and t.company_id = $1 and a.number = '1920' and t.month = '2025-04-01'",
    )
    .bind(company)
    .execute(&pool)
    .await
    .unwrap();
}
