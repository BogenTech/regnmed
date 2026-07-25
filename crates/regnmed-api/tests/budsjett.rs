//! Budsjett (#41): et utkast er fritt redigerbart, fastsettelse fryser
//! versjonen (linjer og rad), en revisjon blir versjon 2, og
//! avviksrapporten navngir alltid hvilket budsjett den sammenligner
//! mot — med faktiske tall fra de samme rene summene som resten av
//! rapportene. «Fra fjoråret ±X %» sår linjene fra virkeligheten.
//! Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = router(state.clone())
        .oneshot(builder.body(Body::from(body.unwrap_or_default())).unwrap())
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

fn salg(dato: chrono::NaiveDate, netto: i64, kostnad: i64) -> VoucherDraft {
    let entry = |konto: &str, amount: i64| EntryDraft {
        account_number: konto.into(),
        amount: Ore(amount),
        vat_code: None,
        description: None,
        party_no: None,
        avdeling: None,
        prosjekt: None,
        valuta: None,
    };
    VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: dato,
        description: format!("Drift {dato}"),
        reverses: None,
        entries: vec![
            entry("1920", netto - kostnad),
            entry("3000", -netto),
            entry("6300", kostnad),
        ],
    }
}

#[tokio::test]
async fn budsjett_versjoneres_og_avviket_navngir_versjonen() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Bea Budsjett"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Plan AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1920", "Bank"),
        ("3000", "Salgsinntekt"),
        ("6300", "Leie"),
        ("7770", "Bankgebyr"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }

    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let year = chrono::Datelike::year(&today);
    let date = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap();

    // I fjor: 10 000 i inntekt og 2 000 i kostnad i januar og februar.
    for m in [1u32, 2] {
        regnmed_db::post_voucher(
            &state.pool,
            company,
            &salg(date(year - 1, m, 15), 10_000_00, 2_000_00),
            "test",
        )
        .await
        .unwrap();
    }
    // I år: januar ble bedre enn i fjor, februar dårligere.
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &salg(date(year, 1, 15), 12_000_00, 2_000_00),
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &salg(date(year, 2, 15), 8_000_00, 2_000_00),
        "test",
    )
    .await
    .unwrap();

    let token = idp.token(&sub, "Bea Budsjett");
    let base = format!("/companies/{company}");

    // «Lag budsjett fra fjoråret +10 %».
    let (status, created) = request(
        &state,
        "POST",
        &format!("{base}/budgets"),
        &token,
        Some(
            json!({"year": year, "navn": "Budsjett", "fra_ar": year - 1, "justering_bp": 1000})
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let v1 = created["budget_id"].as_str().unwrap().to_string();

    let (status, budget) =
        request(&state, "GET", &format!("{base}/budgets/{v1}"), &token, None).await;
    assert_eq!(status, StatusCode::OK, "{budget}");
    assert_eq!(budget["budget"]["versjon"], 1);
    assert_eq!(budget["budget"]["status"], "utkast");
    let lines = budget["lines"].as_array().unwrap();
    // 3000 og 6300 for januar og februar — i presentasjonsfortegn,
    // 10 % over fjoråret.
    let line = |konto: &str, maned: i64| {
        lines
            .iter()
            .find(|l| l["account"] == konto && l["maned"] == maned)
            .unwrap_or_else(|| panic!("mangler {konto} måned {maned}"))["belop_ore"]
            .as_i64()
            .unwrap()
    };
    assert_eq!(line("3000", 1), 11_000_00, "inntekt positiv i budsjettet");
    assert_eq!(line("6300", 2), 2_200_00, "kostnad positiv i budsjettet");

    // Utkastet er fritt redigerbart: erstatt linjene helt.
    let mut nye = Vec::new();
    for maned in 1..=12 {
        nye.push(json!({"account": "3000", "maned": maned, "belop_ore": 10_000_00}));
        nye.push(json!({"account": "6300", "maned": maned, "belop_ore": 2_000_00}));
    }
    let (status, body) = request(
        &state,
        "PUT",
        &format!("{base}/budgets/{v1}/lines"),
        &token,
        Some(json!({"lines": nye}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Balansekontoer hører ikke hjemme i et resultatbudsjett.
    let (status, body) = request(
        &state,
        "PUT",
        &format!("{base}/budgets/{v1}/lines"),
        &token,
        Some(json!({"lines": [{"account": "1920", "maned": 1, "belop_ore": 100}]}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("resultatkonto"),
        "{body}"
    );

    // Fastsettelse fryser versjonen.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/budgets/{v1}/fastsett"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = request(
        &state,
        "PUT",
        &format!("{base}/budgets/{v1}/lines"),
        &token,
        Some(json!({"lines": []}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "fastsatt = frosset");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("ny versjon"),
        "{body}"
    );
    let (status, _) = request(
        &state,
        "DELETE",
        &format!("{base}/budgets/{v1}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "fastsatt slettes ikke");

    // Avviksrapporten navngir versjonen den måler mot.
    let (status, avvik) = request(
        &state,
        "GET",
        &format!("{base}/reports/avvik?year={year}&t_o_m=2"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{avvik}");
    assert_eq!(avvik["budsjett"]["versjon"], 1);
    assert_eq!(avvik["budsjett"]["status"], "fastsatt");
    assert_eq!(avvik["t_o_m_maned"], 2);
    let inntekt = &avvik["seksjoner"][0];
    assert_eq!(inntekt["heading"], "Driftsinntekter");
    assert_eq!(inntekt["budsjett_hittil_ore"], 20_000_00);
    assert_eq!(inntekt["faktisk_hittil_ore"], 20_000_00, "12 000 + 8 000");
    assert_eq!(inntekt["avvik_hittil_ore"], 0);
    assert_eq!(inntekt["budsjett_ar_ore"], 120_000_00);
    assert_eq!(inntekt["linjer"][0]["faktisk_maaneder"][0], 12_000_00);
    assert_eq!(inntekt["linjer"][0]["faktisk_maaneder"][1], 8_000_00);
    // Resultat hittil: (20 000 − 4 000) faktisk mot (20 000 − 4 000) budsjett.
    assert_eq!(avvik["resultat_faktisk_hittil_ore"], 16_000_00);
    assert_eq!(avvik["resultat_budsjett_hittil_ore"], 16_000_00);
    assert_eq!(avvik["resultat_budsjett_ar_ore"], 96_000_00);

    // En revisjon er en NY versjon — den gamle rapporten betyr fortsatt
    // det samme.
    let (status, created) = request(
        &state,
        "POST",
        &format!("{base}/budgets"),
        &token,
        Some(json!({"year": year, "navn": "Budsjett rev. B"}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let v2 = created["budget_id"].as_str().unwrap().to_string();
    let (_, body) = request(
        &state,
        "PUT",
        &format!("{base}/budgets/{v2}/lines"),
        &token,
        Some(
            json!({"lines": [
                {"account": "3000", "maned": 1, "belop_ore": 15_000_00},
                {"account": "3000", "maned": 2, "belop_ore": 15_000_00},
            ]})
            .to_string(),
        ),
    )
    .await;
    assert_eq!(body["lines"], 2);

    // Standardvalget er nyeste FASTSATTE — utkastet v2 overtar ikke
    // rapporten før noen fastsetter det.
    let (_, avvik) = request(
        &state,
        "GET",
        &format!("{base}/reports/avvik?year={year}&t_o_m=2"),
        &token,
        None,
    )
    .await;
    assert_eq!(avvik["budsjett"]["versjon"], 1);
    // …men den kan velges eksplisitt.
    let (_, avvik) = request(
        &state,
        "GET",
        &format!("{base}/reports/avvik?year={year}&t_o_m=2&budget_id={v2}"),
        &token,
        None,
    )
    .await;
    assert_eq!(avvik["budsjett"]["versjon"], 2);
    assert_eq!(avvik["seksjoner"][0]["budsjett_hittil_ore"], 30_000_00);
    assert_eq!(
        avvik["seksjoner"][0]["avvik_hittil_ore"], -10_000_00,
        "20 000 faktisk mot 30 000 planlagt"
    );

    // Et ubudsjettert bilag dukker opp i rapporten, ikke i stillhet.
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: date(year, 2, 20),
            description: "Uventet gebyr".into(),
            reverses: None,
            entries: vec![
                EntryDraft {
                    account_number: "7770".into(),
                    amount: Ore(750_00),
                    vat_code: None,
                    description: None,
                    party_no: None,
                    avdeling: None,
                    prosjekt: None,
                    valuta: None,
                },
                EntryDraft {
                    account_number: "1920".into(),
                    amount: Ore(-750_00),
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
    let (_, avvik) = request(
        &state,
        "GET",
        &format!("{base}/reports/avvik?year={year}&t_o_m=2"),
        &token,
        None,
    )
    .await;
    let annen = avvik["seksjoner"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["heading"] == "Annen driftskostnad")
        .unwrap();
    let gebyr = annen["linjer"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["account"] == "7770")
        .unwrap();
    assert_eq!(gebyr["budsjett_hittil_ore"], 0);
    assert_eq!(gebyr["faktisk_hittil_ore"], 750_00);
    assert_eq!(gebyr["avvik_hittil_ore"], 750_00);

    // Utkast kan forkastes.
    let (status, _) = request(
        &state,
        "DELETE",
        &format!("{base}/budgets/{v2}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, budgets) = request(&state, "GET", &format!("{base}/budgets"), &token, None).await;
    assert_eq!(budgets["budgets"].as_array().unwrap().len(), 1);
}
