//! Periodisering end to end (#87): a prepaid cost is spread over the
//! months it belongs to, the parts sum exactly to the total, a month
//! cannot be posted twice, and stopping leaves what was posted alone.
//! Requires DATABASE_URL (skips otherwise).

use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use regnmed_db::periodisering::{PeriodiseringDraft, create_periodisering, periodiser_plan};
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

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn saldo(pool: &PgPool, company: Uuid, number: &str) -> i64 {
    sqlx::query_scalar::<_, Option<i64>>(
        "select sum(e.amount_ore)::bigint from entry e
         join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(number)
    .fetch_one(pool)
    .await
    .unwrap()
    .unwrap_or(0)
}

#[tokio::test]
async fn a_prepaid_cost_is_spread_over_its_months_and_sums_exactly() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnr(), "Periodisering AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [
        ("1700", "Forskuddsbetalt kostnad"),
        ("1920", "Bank"),
        ("6300", "Leie lokale"),
    ] {
        regnmed_db::ensure_account(&pool, company, nr, navn)
            .await
            .unwrap();
    }

    // The source bilag: a year of rent paid in January, parked on 1700.
    // 10 000,01 kr so the split cannot come out even.
    let total = 1_000_001;
    let kilde = regnmed_db::post_voucher(
        &pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: d(2026, 1, 15),
            description: "Husleie 2026, forskuddsbetalt".into(),
            reverses: None,
            entries: vec![
                EntryDraft {
                    account_number: "1700".into(),
                    amount: Ore(total),
                    vat_code: None,
                    description: None,
                    party_no: None,
                    avdeling: None,
                    prosjekt: None,
                    valuta: None,
                },
                EntryDraft {
                    account_number: "1920".into(),
                    amount: Ore(-total),
                    vat_code: None,
                    description: None,
                    party_no: None,
                    avdeling: None,
                    prosjekt: None,
                    valuta: None,
                },
            ],
        },
        "test",
    )
    .await
    .unwrap();

    let plan = create_periodisering(
        &pool,
        company,
        &PeriodiseringDraft {
            kilde_voucher: Some(kilde.id),
            beskrivelse: "Husleie 2026".into(),
            resultatkonto: "6300".into(),
            balansekonto: "1700".into(),
            total_ore: total,
            fra: (2026, 1),
            til: (2026, 12),
            avdeling: None,
            prosjekt: None,
            notat: None,
        },
        "test",
    )
    .await
    .unwrap();

    // Run as if it were 31 March: three months are due, nine are not.
    let utfall = periodiser_plan(&pool, company, plan, d(2026, 3, 31))
        .await
        .unwrap();
    assert_eq!(utfall.len(), 3, "{utfall:?}");
    assert!(utfall.iter().all(|u| u.voucher.is_some()), "{utfall:?}");
    assert_eq!(saldo(&pool, company, "6300").await, 249_999, "3 × 833,33");
    assert_eq!(
        saldo(&pool, company, "1700").await,
        total - 249_999,
        "resten står fortsatt parkert i balansen"
    );

    // Running again changes nothing — the partial unique index decides,
    // not a flag we remembered to set.
    let igjen = periodiser_plan(&pool, company, plan, d(2026, 3, 31))
        .await
        .unwrap();
    assert!(igjen.is_empty(), "{igjen:?}");
    assert_eq!(saldo(&pool, company, "6300").await, 249_999);

    // The whole year: the parts sum EXACTLY to the total, and 1700 is
    // emptied to the øre — the property the feature stands on.
    periodiser_plan(&pool, company, plan, d(2026, 12, 31))
        .await
        .unwrap();
    assert_eq!(saldo(&pool, company, "6300").await, total);
    assert_eq!(saldo(&pool, company, "1700").await, 0, "på øret");

    regnmed_db::verify_chain(&pool, company).await.unwrap();
}

#[tokio::test]
async fn stopping_leaves_posted_months_alone_and_posts_no_more() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnr(), "Stopp AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [("1700", "Forskuddsbetalt"), ("6300", "Leie")] {
        regnmed_db::ensure_account(&pool, company, nr, navn)
            .await
            .unwrap();
    }
    let plan = create_periodisering(
        &pool,
        company,
        &PeriodiseringDraft {
            kilde_voucher: None,
            beskrivelse: "Forsikring".into(),
            resultatkonto: "6300".into(),
            balansekonto: "1700".into(),
            total_ore: 120_000,
            fra: (2026, 1),
            til: (2026, 12),
            avdeling: None,
            prosjekt: None,
            notat: None,
        },
        "test",
    )
    .await
    .unwrap();
    periodiser_plan(&pool, company, plan, d(2026, 2, 28))
        .await
        .unwrap();
    assert_eq!(saldo(&pool, company, "6300").await, 20_000);

    regnmed_db::periodisering::stopp_periodisering(&pool, company, plan, d(2026, 3, 1))
        .await
        .unwrap();
    // Even asked for the whole year, nothing more is posted.
    let etter = periodiser_plan(&pool, company, plan, d(2026, 12, 31))
        .await
        .unwrap();
    assert!(etter.is_empty(), "{etter:?}");
    assert_eq!(
        saldo(&pool, company, "6300").await,
        20_000,
        "det som er ført står"
    );
}

/// A started plan is history: the amounts already posted refer to it, so
/// changing the total afterwards would break the sum. Enforced by the
/// trigger, not only by the code.
#[tokio::test]
async fn a_started_plan_cannot_be_rewritten() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnr(), "Frys AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [("1700", "Forskuddsbetalt"), ("6300", "Leie")] {
        regnmed_db::ensure_account(&pool, company, nr, navn)
            .await
            .unwrap();
    }
    let plan = create_periodisering(
        &pool,
        company,
        &PeriodiseringDraft {
            kilde_voucher: None,
            beskrivelse: "Abonnement".into(),
            resultatkonto: "6300".into(),
            balansekonto: "1700".into(),
            total_ore: 60_000,
            fra: (2026, 1),
            til: (2026, 6),
            avdeling: None,
            prosjekt: None,
            notat: None,
        },
        "test",
    )
    .await
    .unwrap();
    periodiser_plan(&pool, company, plan, d(2026, 1, 31))
        .await
        .unwrap();

    let feil = sqlx::query("update periodisering set total_ore = 99_000 where id = $1")
        .bind(plan)
        .execute(&pool)
        .await
        .expect_err("en påbegynt plan skal ikke kunne endres");
    assert!(
        feil.to_string().contains("påbegynt"),
        "feilmeldingen skal si hvorfor: {feil}"
    );

    // …but stopping it is still allowed, or a plan could never be ended.
    regnmed_db::periodisering::stopp_periodisering(&pool, company, plan, d(2026, 2, 1))
        .await
        .unwrap();
}

/// A plan is a standing instruction, so a bad account must fail when it
/// is created — not once a month for a year, where only the run log
/// would see it. Found in browser verification (#87).
#[tokio::test]
async fn a_plan_refuses_an_account_it_could_never_post_to() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnr(), "Kontosjekk AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [
        ("1500", "Kundefordringer"),
        ("1700", "Forskuddsbetalt"),
        ("6300", "Leie"),
    ] {
        regnmed_db::ensure_account(&pool, company, nr, navn)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&pool, company, "1500", Some("kunde"))
        .await
        .unwrap();

    let plan = |resultat: &str, balanse: &str| PeriodiseringDraft {
        kilde_voucher: None,
        beskrivelse: "Husleie".into(),
        resultatkonto: resultat.into(),
        balansekonto: balanse.into(),
        total_ore: 30_000,
        fra: (2026, 1),
        til: (2026, 3),
        avdeling: None,
        prosjekt: None,
        notat: None,
    };

    let feil = create_periodisering(&pool, company, &plan("6300", "1500"), "test")
        .await
        .expect_err("reskontrokonto kan aldri lykkes uten part");
    assert!(feil.to_string().contains("reskontrokonto"), "{feil}");

    let feil = create_periodisering(&pool, company, &plan("6300", "9999"), "test")
        .await
        .expect_err("ukjent konto");
    assert!(feil.to_string().contains("finnes ikke"), "{feil}");

    // The good one still works.
    create_periodisering(&pool, company, &plan("6300", "1700"), "test")
        .await
        .unwrap();
}
