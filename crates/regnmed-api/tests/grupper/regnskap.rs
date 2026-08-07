//! Lovpålagte spesifikasjoner over the web API: saldobalanse carries
//! inngående/utgående across a period boundary, kontospesifikasjon has
//! running saldo and dokumentasjonshenvisning, bokføringsspesifikasjon
//! lists vouchers in posting order, and resultat/balanse balance to the
//! øre. Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, gi_partene_adresse, gjor_fakturaklar, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::NaiveDate;
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
        avdeling: None,
        prosjekt: None,
        valuta: None,
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
    gjor_fakturaklar(&state.pool, company).await;
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

fn party_entry(account: &str, ore: i64, party_no: &str) -> EntryDraft {
    EntryDraft {
        party_no: Some(party_no.into()),
        ..entry(account, ore)
    }
}

#[tokio::test]
async fn reskontrospesifikasjon_carries_saldo_per_party() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Reidun Reskontro"), None)
        .await
        .unwrap();
    let stranger_sub = format!("test|{}", Uuid::new_v4());
    regnmed_db::ensure_person(&state.pool, &stranger_sub, Some("Fremmed"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Reskontro AS")
        .await
        .unwrap();
    gjor_fakturaklar(&state.pool, company).await;
    regnmed_db::ensure_company_member(&state.pool, company, person, "les")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("1920", "Bank"),
        ("2400", "Leverandørgjeld"),
        ("3000", "Salgsinntekt"),
        ("4300", "Varekjøp"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    regnmed_db::set_account_reskontro(&state.pool, company, "2400", Some("leverandor"))
        .await
        .unwrap();
    let (_, kunde_a) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunde A", None, None)
            .await
            .unwrap();
    gi_partene_adresse(&state.pool, company).await;
    let (_, kunde_b) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunde B", None, None)
            .await
            .unwrap();
    gi_partene_adresse(&state.pool, company).await;
    let (_, leverandor) =
        regnmed_db::create_party(&state.pool, company, "leverandor", "Grossisten", None, None)
            .await
            .unwrap();
    let token = idp.token(&sub, "Reidun Reskontro");

    // 2025: both kunder invoiced. 2026: A pays and buys again, B is
    // untouched; one supplier invoice arrives.
    post(
        &state.pool,
        company,
        date(2025, 12, 15),
        "Salg A",
        vec![
            party_entry("1500", 5_000_00, &kunde_a),
            entry("3000", -5_000_00),
        ],
    )
    .await;
    post(
        &state.pool,
        company,
        date(2025, 12, 20),
        "Salg B",
        vec![
            party_entry("1500", 2_000_00, &kunde_b),
            entry("3000", -2_000_00),
        ],
    )
    .await;
    post(
        &state.pool,
        company,
        date(2026, 1, 10),
        "Innbetaling A",
        vec![
            entry("1920", 5_000_00),
            party_entry("1500", -5_000_00, &kunde_a),
        ],
    )
    .await;
    post(
        &state.pool,
        company,
        date(2026, 1, 15),
        "Varekjøp",
        vec![
            entry("4300", 8_000_00),
            party_entry("2400", -8_000_00, &leverandor),
        ],
    )
    .await;
    post(
        &state.pool,
        company,
        date(2026, 2, 1),
        "Salg A igjen",
        vec![
            party_entry("1500", 3_000_00, &kunde_a),
            entry("3000", -3_000_00),
        ],
    )
    .await;

    let base = format!("/companies/{company}/reports");
    let (status, ks) = get(
        &state,
        &format!("{base}/kundespesifikasjon?from=2026-01-01&to=2026-12-31"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ks}");
    let parties = ks["parties"].as_array().unwrap();
    // Only kunder — the supplier must not leak into this report.
    assert_eq!(parties.len(), 2);
    let a = &parties[0];
    assert_eq!(a["party_no"], kunde_a);
    assert_eq!(a["inngaende_ore"], 5_000_00);
    assert_eq!(a["utgaende_ore"], 3_000_00);
    let posts = a["posts"].as_array().unwrap();
    assert_eq!(posts.len(), 2);
    // Running saldo per party, seeded from inngående, with the
    // bilagshenvisning the forskrift requires.
    assert_eq!(posts[0]["bilag"], "GL-2026-1");
    assert_eq!(posts[0]["account"], "1500");
    assert_eq!(posts[0]["amount_ore"], -5_000_00);
    assert_eq!(posts[0]["saldo_ore"], 0);
    assert_eq!(posts[1]["amount_ore"], 3_000_00);
    assert_eq!(posts[1]["saldo_ore"], 3_000_00);
    // B has no movement in the period, but the saldo exists and must be
    // in the spesifikasjon.
    let b = &parties[1];
    assert_eq!(b["party_no"], kunde_b);
    assert_eq!(b["inngaende_ore"], 2_000_00);
    assert_eq!(b["utgaende_ore"], 2_000_00);
    assert!(b["posts"].as_array().unwrap().is_empty());

    let (status, ls) = get(
        &state,
        &format!("{base}/leverandorspesifikasjon?from=2026-01-01&to=2026-12-31"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ls}");
    let parties = ls["parties"].as_array().unwrap();
    assert_eq!(parties.len(), 1);
    let g = &parties[0];
    assert_eq!(g["party_no"], leverandor);
    assert_eq!(g["inngaende_ore"], 0);
    assert_eq!(g["utgaende_ore"], -8_000_00);
    let posts = g["posts"].as_array().unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0]["account"], "2400");
    assert_eq!(posts[0]["saldo_ore"], -8_000_00);

    // Guards: no access → 404; nonsense period → 400.
    let stranger_token = idp.token(&stranger_sub, "Fremmed");
    let (status, _) = get(
        &state,
        &format!("{base}/leverandorspesifikasjon?from=2026-01-01&to=2026-12-31"),
        &stranger_token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get(
        &state,
        &format!("{base}/kundespesifikasjon?from=2026-12-31&to=2026-01-01"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
