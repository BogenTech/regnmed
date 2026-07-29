//! Payroll runs against a real Postgres (#46, docs/lonn.md).
//!
//! What must hold: the bilag balances, the forskuddstrekk follows the
//! rules (feriepenger trekkfrie, half tax in December), the
//! arbeidsgiveravgift comes from the satsregister, feriepenger paid out
//! draw down the LIABILITY instead of becoming a new cost, the same month
//! cannot be run twice, and a run cannot be changed afterwards.
//!
//! Requires DATABASE_URL; skips otherwise.

use chrono::NaiveDate;
use regnmed_db::lonn::{self, Lonnspost, NyAnsatt};
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

fn dato(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

async fn selskap(pool: &PgPool) -> Uuid {
    let company = regnmed_db::create_company(pool, &unique_orgnr(), "Lønnstest AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [
        ("5000", "Lønn til ansatte"),
        ("5090", "Feriepenger"),
        ("5400", "Arbeidsgiveravgift"),
        ("2600", "Forskuddstrekk"),
        ("2770", "Skyldig arbeidsgiveravgift"),
        ("2930", "Skyldig lønn"),
        ("2940", "Skyldige feriepenger"),
        ("1920", "Bankinnskudd"),
    ] {
        regnmed_db::ensure_account(pool, company, nr, navn)
            .await
            .unwrap();
    }
    company
}

async fn ansatt(pool: &PgPool, company: Uuid, fnr: &str, navn: &str, lonn: i64) -> Uuid {
    lonn::create_ansatt(
        pool,
        company,
        &NyAnsatt {
            fodselsnummer: fnr.into(),
            navn: navn.into(),
            stilling: Some("Utvikler".into()),
            ansatt_fra: dato(2025, 1, 1),
            manedslonn_ore: Some(lonn),
            timelonn_ore: None,
            trekk_type: "prosent".into(),
            trekk_prosent_bp: Some(3500),
            trekk_tabell: None,
            feriepenger_bp: 1020,
            bank_account: None,
            note: None,
        },
        "Test",
    )
    .await
    .unwrap()
}

/// Sum of a voucher's entries — must be exactly zero, always.
async fn voucher_sum(pool: &PgPool, voucher_id: Uuid) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(amount_ore), 0)::bigint from entry where voucher_id = $1",
    )
    .bind(voucher_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn konto_belop(pool: &PgPool, voucher_id: Uuid, konto: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint from entry e
         join account a on a.id = e.account_id
         where e.voucher_id = $1 and a.number = $2",
    )
    .bind(voucher_id)
    .bind(konto)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_payroll_run_posts_as_one_balanced_bilag() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    // 50 000 kr, 35 % trekk, sone I (14,1 %).
    let a = ansatt(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;

    let kjoring = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        3,
        dato(2026, 3, 25),
        "I",
        &[Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap();

    assert_eq!(kjoring.sum.brutto_ore, 5_000_000);
    assert_eq!(kjoring.sum.forskuddstrekk_ore, 1_750_000);
    assert_eq!(kjoring.sum.netto_ore, 3_250_000);
    // 10,2 % feriepengeavsetning of the gross pay.
    assert_eq!(kjoring.sum.feriepengeavsetning_ore, 510_000);
    // 14,1 % arbeidsgiveravgift, from the satsregister — not from the code.
    assert_eq!(kjoring.sum.aga_ore, 705_000);

    // The bilag balances. Nothing else matters if this fails.
    assert_eq!(voucher_sum(&pool, kjoring.voucher_id).await, 0);

    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "5000").await,
        5_000_000
    );
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "2600").await,
        -1_750_000
    );
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "2930").await,
        -3_250_000
    );
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "5090").await,
        510_000
    );
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "2940").await,
        -510_000
    );
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "5400").await,
        705_000
    );
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "2770").await,
        -705_000
    );
}

/// Feriepenger paid out are not a new cost — they draw down the liability
/// accrued in the year they were earned. Get this wrong and feriepenger
/// are expensed twice, making the result systematically too low.
#[tokio::test]
async fn feriepenger_paid_out_reduce_the_liability_and_carry_no_withholding() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "08888797336", "Ola Ferierende", 3_000_000).await;

    let kjoring = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        6,
        dato(2026, 6, 20),
        "I",
        &[Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 4_000_000,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap();

    // Withhold from ordinary pay only: 35 % of 30 000, not of 70 000.
    assert_eq!(kjoring.sum.forskuddstrekk_ore, 1_050_000);
    assert_eq!(kjoring.sum.netto_ore, 3_000_000 + 4_000_000 - 1_050_000);
    assert_eq!(voucher_sum(&pool, kjoring.voucher_id).await, 0);

    // 5000 carries ONLY ordinary pay — the feriepenger are not a new cost.
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "5000").await,
        3_000_000
    );
    // 2940 is debited 4 000 000 (drawdown) and credited 306 000 (new accrual).
    assert_eq!(
        konto_belop(&pool, kjoring.voucher_id, "2940").await,
        4_000_000 - 306_000
    );
    // The avgift falls on everything actually paid out, feriepenger included.
    assert_eq!(kjoring.sum.aga_ore, 987_000); // 14,1 % av 70 000
}

#[tokio::test]
async fn december_withholds_half() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "25927898821", "Nils Desember", 5_000_000).await;

    let kjoring = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        12,
        dato(2026, 12, 15),
        "I",
        &[Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(
        kjoring.sum.forskuddstrekk_ore, 875_000,
        "halv skatt i desember"
    );
    assert_eq!(voucher_sum(&pool, kjoring.voucher_id).await, 0);
}

#[tokio::test]
async fn sone_v_is_a_zero_rate_and_sone_ia_is_refused() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "26829398612", "Finnmarking", 4_000_000).await;

    let kjoring = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        4,
        dato(2026, 4, 25),
        "V",
        &[Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(kjoring.sum.aga_ore, 0, "sone V er nullsats");
    assert_eq!(voucher_sum(&pool, kjoring.voucher_id).await, 0);

    // Sone Ia is refused: the fribeløp cannot be read out of a single rate.
    let feil = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        5,
        dato(2026, 5, 25),
        "Ia",
        &[Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap_err();
    assert!(feil.to_string().contains("fribeløpet"), "{feil}");
}

#[tokio::test]
async fn tabelltrekk_stops_the_run_rather_than_guessing() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = lonn::create_ansatt(
        &pool,
        company,
        &NyAnsatt {
            fodselsnummer: "08888797336".into(),
            navn: "Tabell Trekksen".into(),
            stilling: None,
            ansatt_fra: dato(2025, 1, 1),
            manedslonn_ore: Some(5_000_000),
            timelonn_ore: None,
            trekk_type: "tabell".into(),
            trekk_prosent_bp: None,
            trekk_tabell: Some(7100),
            feriepenger_bp: 1020,
            bank_account: None,
            note: None,
        },
        "Test",
    )
    .await
    .unwrap();

    let feil = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        3,
        dato(2026, 3, 25),
        "I",
        &[Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap_err();
    assert!(feil.to_string().contains("tabelltrekk"), "{feil}");
    assert!(feil.to_string().contains("tilnærmer dem ikke"), "{feil}");
}

#[tokio::test]
async fn the_same_month_cannot_be_run_twice_and_runs_are_immutable() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "26829398612", "Kari", 4_000_000).await;
    let post = || {
        vec![Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: false,
        }]
    };

    let forste = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        7,
        dato(2026, 7, 25),
        "I",
        &post(),
        "Test",
    )
    .await
    .unwrap();
    assert!(lonn::kjort_maned(&pool, company, 2026, 7).await.unwrap());

    let feil = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        7,
        dato(2026, 7, 25),
        "I",
        &post(),
        "Test",
    )
    .await
    .unwrap_err();
    assert!(feil.to_string().contains("allerede kjørt"), "{feil}");

    // The database itself refuses to change or delete a run.
    let err = sqlx::query("update payroll_run set brutto_ore = 1 where id = $1")
        .bind(forste.id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("innsettings-bare"), "{err}");

    let err = sqlx::query("delete from payroll_line where run_id = $1")
        .bind(forste.id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("innsettings-bare"), "{err}");

    // The employee's identity is not editable either.
    let err = sqlx::query("update employee set fodselsnummer = '08888797336' where id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("uforanderlig"), "{err}");
}

/// The list shows the birth date, not the fødselsnummer — the same
/// privacy choice as in the aksjeeierbok.
#[tokio::test]
async fn the_employee_list_shows_the_birth_date_not_the_fodselsnummer() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    ansatt(&pool, company, "26829398612", "Kari Utvikler", 4_000_000).await;

    let ansatte = lonn::list_ansatte(&pool, company).await.unwrap();
    assert_eq!(ansatte.len(), 1);
    assert_eq!(ansatte[0].fodselsdato, Some(dato(1993, 2, 26)));
    assert!(
        !format!("{:?}", ansatte[0]).contains("26829398612"),
        "fødselsnummeret skal ikke ligge i ansattlisten"
    );
}

#[tokio::test]
async fn an_invalid_fodselsnummer_is_rejected_at_registration() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let feil = lonn::create_ansatt(
        &pool,
        company,
        &NyAnsatt {
            fodselsnummer: "26829398613".into(),
            navn: "Feil Nummer".into(),
            stilling: None,
            ansatt_fra: dato(2025, 1, 1),
            manedslonn_ore: Some(1),
            timelonn_ore: None,
            trekk_type: "ingen".into(),
            trekk_prosent_bp: None,
            trekk_tabell: None,
            feriepenger_bp: 1020,
            bank_account: None,
            note: None,
        },
        "Test",
    )
    .await
    .unwrap_err();
    assert!(feil.to_string().contains("fødselsnummer"), "{feil}");
}

/// The payslip is built from the insert-only payroll line, so it can be
/// reproduced forever — and it must explain the withholding, not merely
/// state it.
#[tokio::test]
async fn the_payslip_is_built_from_the_line_with_year_to_date() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;
    let post = |fp: i64| {
        vec![Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: fp,
            fra_timer: false,
        }]
    };

    lonn::kjor_lonn(
        &pool,
        company,
        2026,
        5,
        dato(2026, 5, 25),
        "I",
        &post(0),
        "Test",
    )
    .await
    .unwrap();
    let juni = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        6,
        dato(2026, 6, 20),
        "I",
        &post(4_000_000),
        "Test",
    )
    .await
    .unwrap();

    let slipp = lonn::lonnsslipp(&pool, company, juni.id, a).await.unwrap();
    assert_eq!(slipp.ansatt_navn, "Kari Utvikler");
    // Birth date, not fødselsnummer — on the payslip too.
    assert_eq!(slipp.ansatt_fodselsdato, Some(dato(1993, 2, 26)));
    // Brutto on the slip is EVERYTHING paid out; the trekkgrunnlag is less.
    assert_eq!(slipp.brutto_ore, 9_000_000);
    assert_eq!(slipp.trekkgrunnlag_ore, 5_000_000);
    assert_eq!(slipp.forskuddstrekk_ore, 1_750_000);
    assert_eq!(slipp.netto_ore, 7_250_000);
    assert_eq!(slipp.trekk_prosent_bp, Some(3500));
    // Year to date through June: two months of pay + June's feriepenger.
    assert_eq!(slipp.hittil_brutto_ore, 5_000_000 + 9_000_000);
    assert_eq!(slipp.hittil_trekk_ore, 3_500_000);
    assert_eq!(slipp.hittil_feriepenger_ore, 1_020_000);

    // And it renders to a PDF that explains the trekkfrihet.
    let pdf = regnmed_core::lonnsslipp::render_lonnsslipp(&slipp);
    assert!(pdf.starts_with(b"%PDF-1.4"));
    let tekst = String::from_utf8_lossy(&pdf).to_string();
    assert!(tekst.contains("uten forskuddstrekk"), "{tekst}");
    assert!(!tekst.contains("26829398612"), "fnr skal ikke i slippen");

    // The May slip sees only May in the year-to-date figures.
    let mai_id = lonn::list_kjoringer(&pool, company, Some(2026))
        .await
        .unwrap()
        .into_iter()
        .find(|k| k.maned == 5)
        .unwrap();
    assert_eq!(mai_id.ansatte.len(), 1, "listingen kjenner deltakerne");
    let mai = lonn::lonnsslipp(&pool, company, mai_id.id, a)
        .await
        .unwrap();
    assert_eq!(mai.hittil_brutto_ore, 5_000_000);
}

/// Hourly pay from the timesheet. The important part here is the
/// REFUSAL: hours that can still be changed must not be payable, because
/// the payroll run is insert-only and the two would then diverge
/// forever.
#[tokio::test]
async fn hourly_pay_requires_the_month_to_be_locked() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;

    let person = regnmed_db::ensure_person(
        &pool,
        &format!("test|{}", Uuid::new_v4()),
        Some("Timelønnet"),
        None,
    )
    .await
    .unwrap();
    let a = lonn::create_ansatt(
        &pool,
        company,
        &NyAnsatt {
            fodselsnummer: "25927898821".into(),
            navn: "Timelønnet".into(),
            stilling: None,
            ansatt_fra: dato(2025, 1, 1),
            manedslonn_ore: None,
            timelonn_ore: Some(45_000), // 450 kr/t
            trekk_type: "prosent".into(),
            trekk_prosent_bp: Some(3000),
            trekk_tabell: None,
            feriepenger_bp: 1020,
            bank_account: None,
            note: None,
        },
        "Test",
    )
    .await
    .unwrap();
    sqlx::query("update employee set person_id = $2 where id = $1")
        .bind(a)
        .bind(person)
        .execute(&pool)
        .await
        .unwrap();

    // 20 hours in March.
    for dag in [2u32, 3, 4] {
        regnmed_db::timesheet::create_time_entry(
            &pool,
            company,
            person,
            &regnmed_db::timesheet::TimeEntryDraft {
                dato: dato(2026, 3, dag),
                minutter: 400,
                beskrivelse: "Arbeid".into(),
                prosjekt: None,
                fakturerbar: false,
                timesats_ore: None,
            },
            "Test",
        )
        .await
        .unwrap();
    }

    let g = lonn::timegrunnlag(&pool, company, a, 2026, 3)
        .await
        .unwrap();
    assert_eq!(g.minutter, 1200, "20 timer");
    assert_eq!(g.belop_ore, 900_000, "20 t x 450 kr = 9 000 kr");
    assert!(!g.laast, "ikke låst ennå");

    let post = || {
        vec![Lonnspost {
            employee_id: a,
            brutto_ore: None,
            feriepenger_ore: 0,
            fra_timer: true,
        }]
    };

    // Unlocked: refused.
    let feil = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        3,
        dato(2026, 3, 25),
        "I",
        &post(),
        "Test",
    )
    .await
    .unwrap_err();
    assert!(feil.to_string().contains("ikke låst"), "{feil}");

    // Lock the month, and the run goes through.
    regnmed_db::timesheet::set_timesheet_lock(&pool, company, dato(2026, 3, 31), "Test", None)
        .await
        .unwrap();
    let g = lonn::timegrunnlag(&pool, company, a, 2026, 3)
        .await
        .unwrap();
    assert!(g.laast);

    let kjoring = lonn::kjor_lonn(
        &pool,
        company,
        2026,
        3,
        dato(2026, 3, 25),
        "I",
        &post(),
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(kjoring.sum.brutto_ore, 900_000, "timene, ikke månedslønn");
    assert_eq!(kjoring.sum.forskuddstrekk_ore, 270_000, "30 % av 9 000 kr");
    assert_eq!(voucher_sum(&pool, kjoring.voucher_id).await, 0);
}

/// Without a link to a portal user the timesheet does not know who the
/// employee is — and then we say so, instead of paying zero.
#[tokio::test]
async fn an_employee_without_a_portal_user_fails_clearly() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "26829398612", "Uten kobling", 4_000_000).await;

    let feil = lonn::timegrunnlag(&pool, company, a, 2026, 3)
        .await
        .unwrap_err();
    assert!(feil.to_string().contains("portalbruker"), "{feil}");
}

// ---------------------------------------------------------------------
// Arbeidsgiveravgift on feriepenger that are accrued but not paid out.
//
// The avgift falls due only when the feriepenger are paid, but the
// obligation arises with the earning. The model is a TARGET, not a stream
// of increments: after every run, account 2780 must be the rate times
// what is actually owed, and the run books the difference.
// ---------------------------------------------------------------------

/// The balance of an account for the WHOLE company, not just one bilag.
async fn konto_saldo(pool: &PgPool, company: Uuid, konto: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint from entry e
         join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(konto)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn kjor(
    pool: &PgPool,
    company: Uuid,
    a: Uuid,
    maned: u32,
    sone: &str,
    brutto: Option<i64>,
    feriepenger: i64,
) -> lonn::Lonnskjoring {
    lonn::kjor_lonn(
        pool,
        company,
        2026,
        maned,
        dato(2026, maned, 20),
        sone,
        &[Lonnspost {
            employee_id: a,
            brutto_ore: brutto,
            feriepenger_ore: feriepenger,
            fra_timer: false,
        }],
        "Test",
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn the_accrual_on_unpaid_feriepenger_is_posted() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "26829398612", "Kari Avsetning", 5_000_000).await;

    let k = kjor(&pool, company, a, 3, "I", None, 0).await;

    // 10,2 % of 50 000 = 5 100 kr of feriepenger owed; 14,1 % of that is
    // 719,10 kr of avgift accruing now and falling due on payout.
    assert_eq!(k.sum.feriepengeavsetning_ore, 510_000);
    assert_eq!(k.sum.aga_feriepenger_ore, 71_910);
    assert_eq!(voucher_sum(&pool, k.voucher_id).await, 0);
    assert_eq!(konto_belop(&pool, k.voucher_id, "5405").await, 71_910);
    assert_eq!(konto_belop(&pool, k.voucher_id, "2780").await, -71_910);
    assert!(k.advarsler.is_empty(), "{:?}", k.advarsler);
}

/// The life cycle: the avgift is accrued on earning and reversed on
/// payout — because then the ordinary aga line is what carries it. Get
/// this wrong and the avgift is either expensed twice or left standing as
/// a liability that never goes away.
#[tokio::test]
async fn the_accrual_is_reversed_when_the_feriepenger_are_paid_out() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "08888797336", "Ola Feriepenger", 5_000_000).await;

    let opptjening = kjor(&pool, company, a, 3, "I", None, 0).await;
    assert_eq!(opptjening.sum.aga_feriepenger_ore, 71_910);

    // Holiday taken: no ordinary pay, the feriepenger are paid out.
    let utbetaling = kjor(&pool, company, a, 4, "I", Some(0), 510_000).await;

    assert_eq!(
        utbetaling.sum.aga_feriepenger_ore, -71_910,
        "avsetningen føres tilbake i sin helhet"
    );
    assert_eq!(voucher_sum(&pool, utbetaling.voucher_id).await, 0);
    // The avgift on what was paid out now sits in the ordinary aga line.
    assert_eq!(utbetaling.sum.aga_ore, 71_910);

    // And afterwards both accounts stand at zero: no liability left, no
    // accrual left.
    assert_eq!(konto_saldo(&pool, company, "2940").await, 0);
    assert_eq!(konto_saldo(&pool, company, "2780").await, 0);
}

/// A feriepenger liability that carries no accrual — because it was
/// earned before the function existed, or in a zone without avgift — is
/// caught up on the next run. That is the whole point of aiming at a
/// balance instead of adding an amount.
#[tokio::test]
async fn a_liability_without_an_accrual_is_caught_up_on_the_next_run() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "25927898821", "Nils Sonebytte", 5_000_000).await;

    // Sone V is a zero rate: feriepenger are earned, no avgift is accrued.
    let uten = kjor(&pool, company, a, 3, "V", None, 0).await;
    assert_eq!(uten.sum.aga_feriepenger_ore, 0);
    assert_eq!(konto_saldo(&pool, company, "2780").await, 0);

    // The business moves to sone I. Now it owes avgift on EVERYTHING
    // outstanding, not only on this month's earning.
    let med = kjor(&pool, company, a, 4, "I", None, 0).await;
    let skyldig = 510_000 + 510_000;
    assert_eq!(
        med.sum.aga_feriepenger_ore,
        skyldig * 1410 / 10_000,
        "hele gjelden får avsetning, ikke bare den nye måneden"
    );
    assert_eq!(konto_saldo(&pool, company, "2780").await, -143_820);
}

/// The invariant that keeps the accrual from drifting: after any run, the
/// balance of 2780 is exactly the rate times the balance of 2940.
#[tokio::test]
async fn the_accrual_is_always_the_rate_times_the_feriepenger_liability() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "03048810003", "Turid Invariant", 4_321_000).await;
    let b = ansatt(&pool, company, "03048810194", "Per Invariant", 2_777_700).await;

    for (maned, feriepenger) in [(3u32, 0i64), (4, 0), (5, 130_000), (6, 250_000)] {
        lonn::kjor_lonn(
            &pool,
            company,
            2026,
            maned,
            dato(2026, maned, 20),
            "I",
            &[
                Lonnspost {
                    employee_id: a,
                    brutto_ore: None,
                    feriepenger_ore: feriepenger,
                    fra_timer: false,
                },
                Lonnspost {
                    employee_id: b,
                    brutto_ore: None,
                    feriepenger_ore: feriepenger,
                    fra_timer: false,
                },
            ],
            "Test",
        )
        .await
        .unwrap();

        // The rounding happens PER EMPLOYEE, so the expected value must
        // be built per employee — 14,1 % of the total would be off by an
        // øre or two and turn the test into an approximation rather than
        // an invariant.
        let per_ansatt: Vec<i64> = sqlx::query_scalar(
            "select coalesce(sum(l.feriepengeavsetning_ore - l.feriepenger_ore), 0)::bigint
             from payroll_line l join payroll_run r on r.id = l.run_id
             where r.company_id = $1 group by l.employee_id",
        )
        .bind(company)
        .fetch_all(&pool)
        .await
        .unwrap();
        let ventet: i64 = per_ansatt
            .iter()
            .map(|s| regnmed_core::lonn::aga_avsetning_mal(*s, 1410))
            .sum();

        assert_eq!(
            per_ansatt.iter().sum::<i64>(),
            -konto_saldo(&pool, company, "2940").await,
            "etter {maned:02}/2026: lønnshistorikken skal forklare hele 2940"
        );
        assert_eq!(
            -konto_saldo(&pool, company, "2780").await,
            ventet,
            "etter {maned:02}/2026: 2780 skal være 14,1 % av hver ansatts gjeld"
        );
    }
}

/// A feriepenger liability that does not come from the payroll runs — an
/// opening balance, a manual accrual — cannot be tied to any employee,
/// and therefore gets no avgift accrual. That is a real limitation, and
/// the run says so instead of pretending otherwise.
#[tokio::test]
async fn an_unallocated_feriepenger_liability_warns_rather_than_staying_silent() {
    let Some(pool) = pool().await else { return };
    let company = selskap(&pool).await;
    let a = ansatt(&pool, company, "03048810275", "Åse Overtatt", 5_000_000).await;

    // The regnskapsfører accrues feriepenger manually, with no employee link.
    use regnmed_core::Ore;
    use regnmed_core::voucher::{EntryDraft, VoucherDraft};
    let linje = |konto: &str, belop: i64| EntryDraft {
        account_number: konto.into(),
        amount: Ore(belop),
        vat_code: None,
        description: Some("Overtatt feriepengegjeld".into()),
        party_no: None,
        avdeling: None,
        prosjekt: None,
        valuta: None,
    };
    regnmed_db::post_voucher(
        &pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: dato(2026, 1, 1),
            description: "Åpningsbalanse feriepenger".into(),
            reverses: None,
            entries: vec![linje("5090", 1_000_000), linje("2940", -1_000_000)],
        },
        "Test",
    )
    .await
    .unwrap();

    let k = kjor(&pool, company, a, 3, "I", None, 0).await;

    // The accrual covers only what the payroll run itself earned.
    assert_eq!(k.sum.aga_feriepenger_ore, 71_910);
    let advarsel = k.advarsler.join(" ");
    assert!(
        advarsel.contains("10000,00"),
        "differansen skal navngis: {advarsel}"
    );
    assert!(advarsel.contains("2780"), "{advarsel}");
}
