//! Lovpålagte spesifikasjoner over the web API: saldobalanse carries
//! inngående/utgående across a period boundary, kontospesifikasjon has
//! running saldo and dokumentasjonshenvisning, bokføringsspesifikasjon
//! lists vouchers in posting order, and resultat/balanse balance to the
//! øre. Requires DATABASE_URL (skips otherwise).

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

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn entry(account: &str, ore: i64) -> EntryDraft {
    EntryDraft {
        account_number: account.into(),
        amount: Ore(ore),
        vat_code: None,
        description: None,
        party_no: None,
    }
}

async fn post(pool: &sqlx::PgPool, company: Uuid, day: NaiveDate, text: &str, e: Vec<EntryDraft>) {
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: day,
        description: text.into(),
        reverses: None,
        entries: e,
    };
    regnmed_db::post_voucher(pool, company, &draft, "test")
        .await
        .unwrap();
}

async fn get(state: &AppState, uri: &str, bearer: &str) -> (StatusCode, serde_json::Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
        )
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

fn account<'a>(report: &'a serde_json::Value, number: &str) -> &'a serde_json::Value {
    report["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["number"] == number)
        .unwrap_or_else(|| panic!("account {number} in report"))
}

#[tokio::test]
async fn statutory_reports_reconcile_to_the_ore() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Rita Rapport"), None)
        .await
        .unwrap();
    let stranger_sub = format!("test|{}", Uuid::new_v4());
    regnmed_db::ensure_person(&state.pool, &stranger_sub, Some("Fremmed"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Rapport AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "les")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1920", "Bank"),
        ("2000", "Aksjekapital"),
        ("3000", "Salgsinntekt"),
        ("4300", "Varekjøp"),
        ("7770", "Bankgebyr"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let token = idp.token(&sub, "Rita Rapport");

    // 2025: stiftelse. 2026: ett salg, ett varekjøp, ett gebyr.
    post(
        &state.pool,
        company,
        date(2025, 12, 1),
        "Stiftelse",
        vec![entry("1920", 100_000_00), entry("2000", -100_000_00)],
    )
    .await;
    post(
        &state.pool,
        company,
        date(2026, 2, 10),
        "Salg",
        vec![entry("1920", 10_000_00), entry("3000", -10_000_00)],
    )
    .await;
    post(
        &state.pool,
        company,
        date(2026, 3, 5),
        "Varekjøp",
        vec![entry("4300", 8_000_00), entry("1920", -8_000_00)],
    )
    .await;
    post(
        &state.pool,
        company,
        date(2026, 3, 6),
        "Gebyr",
        vec![entry("7770", 150_00), entry("1920", -150_00)],
    )
    .await;

    // Saldobalanse 2026: bank carries inngående from 2025 and splits
    // period movement into debet/kredit.
    let base = format!("/companies/{company}/reports");
    let (status, sb) = get(
        &state,
        &format!("{base}/saldobalanse?from=2026-01-01&to=2026-12-31"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sb}");
    let bank = account(&sb, "1920");
    assert_eq!(bank["inngaende_ore"], 100_000_00);
    assert_eq!(bank["debet_ore"], 10_000_00);
    assert_eq!(bank["kredit_ore"], -8_150_00);
    assert_eq!(bank["utgaende_ore"], 101_850_00);
    // Aksjekapital: no 2026 movement, but the balance must still appear.
    assert_eq!(account(&sb, "2000")["utgaende_ore"], -100_000_00);

    // Kontospesifikasjon for bank: running saldo seeded from inngående,
    // with the bilagshenvisning the forskrift requires.
    let (status, ks) = get(
        &state,
        &format!("{base}/kontospesifikasjon?from=2026-01-01&to=2026-12-31&account=1920"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let posts = ks["posts"].as_array().unwrap();
    assert_eq!(posts.len(), 3);
    assert_eq!(posts[0]["bilag"], "GL-2026-1");
    assert_eq!(posts[0]["saldo_ore"], 110_000_00);
    assert_eq!(posts[1]["saldo_ore"], 102_000_00);
    assert_eq!(posts[2]["saldo_ore"], 101_850_00);

    // Bokføringsspesifikasjon: all three 2026 vouchers in posting order,
    // every voucher balancing to zero.
    let (_, bs) = get(
        &state,
        &format!("{base}/bokforingsspesifikasjon?from=2026-01-01&to=2026-12-31"),
        &token,
    )
    .await;
    let vouchers = bs["vouchers"].as_array().unwrap();
    assert_eq!(vouchers.len(), 3);
    assert_eq!(vouchers[0]["description"], "Salg");
    for voucher in vouchers {
        let sum: i64 = voucher["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["amount_ore"].as_i64().unwrap())
            .sum();
        assert_eq!(sum, 0, "voucher {} balances", voucher["bilag"]);
    }

    // Resultat 2026: inntekter positive, årsresultat 1 850,00.
    let (_, r) = get(
        &state,
        &format!("{base}/resultat?from=2026-01-01&to=2026-12-31"),
        &token,
    )
    .await;
    assert_eq!(r["seksjoner"][0]["sum_ore"], 10_000_00);
    assert_eq!(r["driftsresultat_ore"], 1_850_00);
    assert_eq!(r["arsresultat_ore"], 1_850_00);

    // Balanse per 2026-12-31: balances to the øre via udisponert
    // resultat, and includes the 2025 history.
    let (_, b) = get(&state, &format!("{base}/balanse?date=2026-12-31"), &token).await;
    assert_eq!(b["eiendeler"]["sum_ore"], 101_850_00);
    assert_eq!(b["egenkapital_gjeld"]["sum_ore"], 100_000_00);
    assert_eq!(b["udisponert_resultat_ore"], 1_850_00);
    assert_eq!(b["differanse_ore"], 0);

    // Guards: no access → 404; nonsense period → 400.
    let stranger_token = idp.token(&stranger_sub, "Fremmed");
    let (status, _) = get(
        &state,
        &format!("{base}/saldobalanse?from=2026-01-01&to=2026-12-31"),
        &stranger_token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get(
        &state,
        &format!("{base}/resultat?from=2026-12-31&to=2026-01-01"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
