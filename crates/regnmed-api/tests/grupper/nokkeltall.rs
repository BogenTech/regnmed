//! Key figures (#36): result year to date against the same period last
//! year, month columns, the liquidity picture from hovedbok and reskontro,
//! and upcoming mva deadlines under the company's terminordning — all
//! plain queries. Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, gi_partene_adresse, make_invoiceable, test_state, unique_orgnumber};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};

async fn request(state: &AppState, uri: &str, bearer: &str) -> (StatusCode, serde_json::Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
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

fn entry(account: &str, amount: i64, party: Option<&str>) -> EntryDraft {
    EntryDraft {
        account_number: account.into(),
        amount: Ore(amount),
        vat_code: None,
        description: None,
        party_no: party.map(str::to_string),
        department: None,
        project: None,
        currency: None,
    }
}

fn voucher(date: chrono::NaiveDate, entries: Vec<EntryDraft>) -> VoucherDraft {
    VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: format!("Bilag {date}"),
        reverses: None,
        entries,
    }
}

#[tokio::test]
async fn key_figures_result_liquidity_and_deadlines() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Nøkkel Tall"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Oversikt AS")
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
        ("1500", "Kundefordringer"),
        ("1920", "Bank"),
        ("2400", "Leverandørgjeld"),
        ("3000", "Salgsinntekt"),
        ("4300", "Varekostnad"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_subledger(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    regnmed_db::set_account_subledger(&state.pool, company, "2400", Some("leverandor"))
        .await
        .unwrap();
    let (_, customer) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunden AS", None, None)
            .await
            .unwrap();
    gi_partene_adresse(&state.pool, company).await;
    let (_, supplier) = regnmed_db::create_party(
        &state.pool,
        company,
        "leverandor",
        "Leverandøren AS",
        None,
        None,
    )
    .await
    .unwrap();

    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let year = chrono::Datelike::year(&today);
    let date = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap();

    // Last year: 20 000 of income before the cutoff (Jan), 99 000 AFTER
    // the cutoff (31 Dec) which must not count in "the same period last year".
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &voucher(
            date(year - 1, 1, 15),
            vec![
                entry("1500", 20_000_00, Some(&customer)),
                entry("3000", -20_000_00, None),
            ],
        ),
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &voucher(
            date(year - 1, 12, 31),
            vec![
                entry("1920", 99_000_00, None),
                entry("3000", -99_000_00, None),
            ],
        ),
        "test",
    )
    .await
    .unwrap();
    // I år: salg 50 000 i januar (kunde, åpen fordring), varekjøp
    // 10 000 i februar (leverandør, åpen gjeld), bank 5 000 inn.
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &voucher(
            date(year, 1, 10),
            vec![
                entry("1500", 50_000_00, Some(&customer)),
                entry("3000", -50_000_00, None),
            ],
        ),
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &voucher(
            date(year, 2, 10),
            vec![
                entry("4300", 10_000_00, None),
                entry("2400", -10_000_00, Some(&supplier)),
            ],
        ),
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &voucher(
            date(year, 1, 20),
            vec![
                entry("1920", 5_000_00, None),
                entry("1500", -5_000_00, Some(&customer)),
            ],
        ),
        "test",
    )
    .await
    .unwrap();

    let token = idp.token(&sub, "Nøkkel Tall");
    let (status, tall) = request(
        &state,
        &format!("/companies/{company}/reports/nokkeltall"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {tall}");

    // Result year to date: 50 000 − 10 000; last year to the same date:
    // 20 000 (the December sale does NOT count).
    assert_eq!(tall["resultat_hittil_ore"], 40_000_00);
    assert_eq!(tall["resultat_fjor_ore"], 20_000_00);
    assert_eq!(tall["maaneder"][0], 50_000_00, "januar");
    assert_eq!(tall["maaneder"][1], -10_000_00, "februar er kostnad");
    assert_eq!(tall["maaneder"][11], 0);

    // Likviditet: bank 104 000 (99 000 fra i fjor + 5 000 innbetalt),
    // kunder 65 000 åpent (20+50−5), leverandører 10 000, mva 0 →
    // disponibelt 159 000.
    let likv = &tall["likviditet"];
    assert_eq!(likv["bank_ore"], 104_000_00);
    assert_eq!(likv["kundefordringer_ore"], 65_000_00);
    assert_eq!(likv["leverandorgjeld_ore"], 10_000_00);
    assert_eq!(likv["mva_netto_ore"], 0);
    assert_eq!(likv["disponibelt_ore"], 159_000_00);

    // Deadlines: the next two mva deadlines under the ordning, never in the past.
    let frister = tall["frister"].as_array().unwrap();
    assert_eq!(frister.len(), 2);
    for deadline in frister {
        assert_eq!(deadline["type"], "mva");
        assert!(deadline["frist"].as_str().unwrap() >= today.to_string().as_str());
    }
}
