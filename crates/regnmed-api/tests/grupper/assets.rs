//! Anleggsregister end to end: registration (with aktiveringsgrense
//! warning), monthly lineære avskrivninger as ordinary vouchers
//! (idempotent per period), one-way avhending with gevinst/tap,
//! DB-layer immutability, and the skattemessige saldoberegning that
//! fails loudly outside the satsregister's coverage.
//! Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

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

fn asset_row<'a>(body: &'a serde_json::Value, navn: &str) -> &'a serde_json::Value {
    body["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["navn"] == navn)
        .unwrap()
}

#[tokio::test]
async fn assets_depreciate_and_dispose() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Anlegg Ansvarlig"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Maskinpark AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1240", "Maskiner"),
        ("6000", "Avskrivninger"),
        ("1920", "Bank"),
        ("3880", "Gevinst ved avgang"),
        ("7880", "Tap ved avgang"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let token = idp.token(&sub, "Anlegg Ansvarlig");
    let base = format!("/companies/{company}/assets");

    // Register over grensen: no warning. Under grensen: warning, never
    // refusal. Unknown gruppe and unknown konto: rejected.
    let (status, made) = request(
        &state,
        "POST",
        &base,
        &token,
        Some(
            serde_json::json!({
                "navn": "Fresemaskin", "anskaffelsesdato": "2026-01-15",
                "kostpris_ore": 36_000_00, "levetid_maneder": 36,
                "balansekonto": "1240", "saldogruppe": "d",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {made}");
    assert!(made["warning"].is_null());
    let fresemaskin = made["asset_id"].as_str().unwrap().to_string();
    let (status, made) = request(
        &state,
        "POST",
        &base,
        &token,
        Some(
            serde_json::json!({
                "navn": "Kontormaskin", "anskaffelsesdato": "2026-02-10",
                "kostpris_ore": 12_000_00, "levetid_maneder": 36,
                "balansekonto": "1240", "saldogruppe": "a",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        made["warning"]
            .as_str()
            .unwrap()
            .contains("aktiveringsgrensen"),
        "under 30 000 kr → warning: {made}"
    );
    let kontormaskin = made["asset_id"].as_str().unwrap().to_string();
    for bad in [
        serde_json::json!({ "navn": "X", "anskaffelsesdato": "2026-01-01", "kostpris_ore": 50_000_00,
            "levetid_maneder": 60, "saldogruppe": "k", "balansekonto": "1240" }),
        serde_json::json!({ "navn": "X", "anskaffelsesdato": "2026-01-01", "kostpris_ore": 50_000_00,
            "levetid_maneder": 60, "saldogruppe": "d", "balansekonto": "9999" }),
    ] {
        let (status, _) = request(&state, "POST", &base, &token, Some(bad.to_string())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // Depreciate through end of April: fresemaskin jan–apr (4 × 1 000),
    // kontormaskin feb–apr (3 × 333,33). Idempotent on the second run.
    let (status, dep) = request(
        &state,
        "POST",
        &format!("{base}/depreciate"),
        &token,
        Some(serde_json::json!({ "through": "2026-04-30" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {dep}");
    assert_eq!(dep["generated"], 7);
    assert_eq!(dep["failed"], 0);
    let (_, dep) = request(
        &state,
        "POST",
        &format!("{base}/depreciate"),
        &token,
        Some(serde_json::json!({ "through": "2026-04-30" }).to_string()),
    )
    .await;
    assert_eq!(dep["generated"], 0, "a period never depreciates twice");

    let (_, listed) = request(&state, "GET", &base, &token, None).await;
    assert_eq!(
        asset_row(&listed, "Fresemaskin")["akkumulert_ore"],
        4_000_00
    );
    assert_eq!(asset_row(&listed, "Fresemaskin")["bokfort_ore"], 32_000_00);
    assert_eq!(asset_row(&listed, "Kontormaskin")["akkumulert_ore"], 99_999);
    let (_, runs) = request(
        &state,
        "GET",
        &format!("{base}/{fresemaskin}/runs"),
        &token,
        None,
    )
    .await;
    assert_eq!(runs["runs"].as_array().unwrap().len(), 4);
    assert!(
        runs["runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["voucher"].is_string())
    );

    // Evidence at the DB layer: no edits, no deletes, run log frozen.
    for tamper in [
        "update asset set kostpris_ore = 1 where company_id = $1",
        "delete from asset where company_id = $1",
    ] {
        let result = sqlx::query(tamper).bind(company).execute(&state.pool).await;
        assert!(result.is_err(), "asset must be immutable: {tamper}");
    }
    let result = sqlx::query(
        "update asset_depreciation set amount_ore = 1
         where asset_id = $1",
    )
    .bind(Uuid::parse_str(&fresemaskin).unwrap())
    .execute(&state.pool)
    .await;
    assert!(result.is_err(), "run log is append-only");

    // Avhending med gevinst: vederlag 33 000 mot bokført 32 000.
    let (status, disposal) = request(
        &state,
        "POST",
        &format!("{base}/{fresemaskin}/dispose"),
        &token,
        Some(serde_json::json!({ "dato": "2026-05-10", "vederlag_ore": 33_000_00 }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {disposal}");
    assert_eq!(disposal["bokfort_ore"], 32_000_00);
    assert_eq!(disposal["gevinst_ore"], 1_000_00);
    assert!(disposal["voucher"].is_string());
    let gevinst_saldo: i64 = sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = '3880'",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(gevinst_saldo, -1_000_00, "gevinsten er kreditert 3880");
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{fresemaskin}/dispose"),
        &token,
        Some(serde_json::json!({ "dato": "2026-05-11", "vederlag_ore": 1 }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "already disposed");
    let (_, dep) = request(
        &state,
        "POST",
        &format!("{base}/depreciate"),
        &token,
        Some(serde_json::json!({ "through": "2026-05-31" }).to_string()),
    )
    .await;
    assert_eq!(
        dep["generated"], 1,
        "kontormaskin får mai; avhendet fresemaskin får ingenting"
    );

    // Utrangering (vederlag 0) posts a pure tap voucher.
    let (status, disposal) = request(
        &state,
        "POST",
        &format!("{base}/{kontormaskin}/dispose"),
        &token,
        Some(serde_json::json!({ "dato": "2026-06-01" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(disposal["bokfort_ore"], 12_000_00 - 133_332);
    assert_eq!(disposal["gevinst_ore"], -(12_000_00 - 133_332));

    // Skattemessig saldo 2026: gruppe a tilgang 12 000 (vederlag 0),
    // gruppe d tilgang 36 000 vederlag 33 000.
    let (status, saldo) = request(
        &state,
        "GET",
        &format!("{base}/saldo?year=2026"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {saldo}");
    let gruppe = |g: &str| {
        saldo["grupper"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["gruppe"] == g)
            .unwrap()
            .clone()
    };
    let a = gruppe("a");
    assert_eq!(a["grunnlag_ore"], 12_000_00);
    assert_eq!(a["sats_bp"], 3000);
    assert_eq!(a["avskrivning_ore"], 3_600_00);
    assert_eq!(a["utgaende_ore"], 8_400_00);
    let d = gruppe("d");
    assert_eq!(d["tilgang_ore"], 36_000_00);
    assert_eq!(d["vederlag_ore"], 33_000_00);
    assert_eq!(d["grunnlag_ore"], 3_000_00);
    assert_eq!(d["avskrivning_ore"], 600_00);
    assert_eq!(d["utgaende_ore"], 2_400_00);
    assert_eq!(saldo["skattemessig_ore"], 10_800_00);
    assert_eq!(saldo["bokfort_ore"], 0, "begge driftsmidlene er avhendet");

    // The whole chain verifies: 8 avskrivninger + 2 avhendinger.
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 10);

    // Outside the satsregister's coverage the report fails loudly.
    let (status, _) = request(
        &state,
        "POST",
        &base,
        &token,
        Some(
            serde_json::json!({
                "navn": "Gammel maskin", "anskaffelsesdato": "2024-06-01",
                "kostpris_ore": 40_000_00, "levetid_maneder": 60,
                "balansekonto": "1240", "saldogruppe": "d",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = request(
        &state,
        "GET",
        &format!("{base}/saldo?year=2026"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "no 2024 rate: {body}");
}
