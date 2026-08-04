//! Dimensjoner end to end: register avdeling/prosjekt, post with codes
//! (hash format v3 — the chain verifies with old and new vouchers
//! mixed), avsluttet prosjekt rejects postings, resultat filters per
//! dimension, and the SAF-T export carries Analysis elements.
//! Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn send(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
) -> (StatusCode, Vec<u8>) {
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
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, bytes.to_vec())
}

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
) -> (StatusCode, serde_json::Value) {
    let (status, bytes) = send(state, method, uri, bearer, body).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

/// A prosjekt may carry its kunde (#80): editable metadata, never part
/// of the chain. Only prosjekter, only actual customers, one at a time.
#[tokio::test]
async fn a_prosjekt_carries_its_kunde() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Dim Admin"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Kundekobling AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let (_, kunde) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunden AS", None, None)
            .await
            .unwrap();
    let (_, leverandor) = regnmed_db::create_party(
        &state.pool,
        company,
        "leverandor",
        "Leverandøren AS",
        None,
        None,
    )
    .await
    .unwrap();
    let token = idp.token(&sub, "Dim Admin");
    let base = format!("/companies/{company}/dimensions");

    // Created with the link; the listing carries number and name.
    let (status, svar) = request(
        &state,
        "POST",
        &base,
        &token,
        Some(serde_json::json!({"kind": "prosjekt", "code": "P9", "name": "Leveransen", "kunde": kunde}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {svar}");
    let (_, liste) = request(&state, "GET", &base, &token, None).await;
    let p9 = liste["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "P9")
        .unwrap();
    assert_eq!(p9["kunde"], kunde.as_str());
    assert_eq!(p9["kunde_navn"], "Kunden AS");

    // An avdeling cannot have a customer; a supplier and an unknown
    // number are not customers.
    for (body, feil) in [
        (
            serde_json::json!({"kind": "avdeling", "code": "A9", "name": "Feil", "kunde": kunde}),
            "bare prosjekter",
        ),
        (
            serde_json::json!({"kind": "prosjekt", "code": "P10", "name": "Feil", "kunde": leverandor}),
            "ingen kunde",
        ),
        (
            serde_json::json!({"kind": "prosjekt", "code": "P11", "name": "Feil", "kunde": "999999"}),
            "ingen kunde",
        ),
    ] {
        let (status, svar) = request(&state, "POST", &base, &token, Some(body.to_string())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {svar}");
        assert!(
            svar["error"].as_str().unwrap_or("").contains(feil),
            "expected «{feil}»: {svar}"
        );
    }

    // The link is editable: cleared with "", relinked with a number —
    // the code itself stays immutable throughout.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/prosjekt/P9"),
        &token,
        Some(serde_json::json!({"kunde": ""}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, liste) = request(&state, "GET", &base, &token, None).await;
    let p9 = liste["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "P9")
        .unwrap();
    assert!(p9["kunde"].is_null(), "cleared: {p9}");
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/prosjekt/P9"),
        &token,
        Some(serde_json::json!({"kunde": kunde}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Prosjektlønnsomhet (#71): the report composes the ledger's
/// dimension-filtered sums with the hours' billed/unbilled split and
/// the kunde link — presentation signs, no stored state.
#[tokio::test]
async fn prosjektlonnsomhet_composes_ledger_and_hours() {
    use regnmed_core::Ore;
    use regnmed_core::voucher::{EntryDraft, VoucherDraft};

    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Prosjekt Per"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Lønnsomhet AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("1920", "Bank"),
        ("3000", "Salgsinntekt"),
        ("2700", "Utgående mva"),
        ("4300", "Varekostnad"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (_, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunden AS", None, None)
            .await
            .unwrap();
    regnmed_db::create_dimension(
        &state.pool,
        company,
        "prosjekt",
        "PL1",
        "Leveransen",
        Some(&party_no),
    )
    .await
    .unwrap();
    let token = idp.token(&sub, "Prosjekt Per");

    // Revenue: an invoice with the prosjekt on the revenue line —
    // 10 000 ex mva.
    let (status, issued) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "invoice_date": "2026-03-01",
                "due_date": "2026-03-15",
                "lines": [{
                    "description": "Leveranse",
                    "unit_price_ore": 10_000_00,
                    "vat_code": "3",
                    "prosjekt": "PL1",
                }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");

    // Cost: 3 000 on the project; the bank leg carries no dimension and
    // must stay out of the project's economics.
    let linje = |konto: &str, belop: i64, prosjekt: Option<&str>| EntryDraft {
        account_number: konto.into(),
        amount: Ore(belop),
        vat_code: None,
        description: Some("Materialkjøp".into()),
        party_no: None,
        avdeling: None,
        prosjekt: prosjekt.map(Into::into),
        valuta: None,
    };
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: chrono::NaiveDate::from_ymd_opt(2026, 3, 10).unwrap(),
            description: "Materialer PL1".into(),
            reverses: None,
            entries: vec![
                linje("4300", 3_000_00, Some("PL1")),
                linje("1920", -3_000_00, None),
            ],
        },
        "Test",
    )
    .await
    .unwrap();

    // Hours: 60 min billed (through the ordinary timesheet invoice,
    // which adds 1 000 revenue on the project), 90 min still on the
    // table. Sats 1 000 kr/t.
    let time = |dato: chrono::NaiveDate| regnmed_db::timesheet::TimeEntryDraft {
        dato,
        minutter: 60,
        beskrivelse: "Prosjektarbeid".into(),
        prosjekt: Some("PL1".into()),
        fakturerbar: Some(true),
        timesats_ore: Some(100_000),
    };
    regnmed_db::timesheet::create_time_entry(
        &state.pool,
        company,
        person,
        &time(chrono::NaiveDate::from_ymd_opt(2026, 4, 1).unwrap()),
        true,
        "Test",
    )
    .await
    .unwrap();
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/timesheet/invoice"),
        &token,
        Some(serde_json::json!({ "party_no": party_no, "prosjekt": "PL1" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut ufakturert = time(chrono::NaiveDate::from_ymd_opt(2026, 4, 2).unwrap());
    ufakturert.minutter = 90;
    regnmed_db::timesheet::create_time_entry(
        &state.pool,
        company,
        person,
        &ufakturert,
        true,
        "Test",
    )
    .await
    .unwrap();

    // The overview: one row for PL1 with everything composed.
    let (status, rapport) = request(
        &state,
        "GET",
        &format!("/companies/{company}/reports/prosjekt?year=2026"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {rapport}");
    let rad = rapport["prosjekter"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["prosjekt"] == "PL1")
        .expect("PL1 in the overview");
    assert_eq!(rad["kunde_navn"], "Kunden AS");
    assert_eq!(rad["inntekter_ore"], 1_100_000, "10 000 + 1 000 timer");
    assert_eq!(rad["kostnader_ore"], 300_000);
    assert_eq!(rad["resultat_ore"], 800_000);
    assert_eq!(rad["timer"]["minutter"], 150);
    assert_eq!(rad["timer"]["fakturerte_minutter"], 60);
    assert_eq!(rad["timer"]["fakturert_ore"], 100_000);
    assert_eq!(rad["timer"]["ufakturert_ore"], 150_000, "på bordet");

    // The detail adds the NS 4102 sections for exactly this project.
    let (status, detalj) = request(
        &state,
        "GET",
        &format!("/companies/{company}/reports/prosjekt?year=2026&prosjekt=PL1"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {detalj}");
    assert_eq!(detalj["resultat_ore"], 800_000);
    let inntekter = detalj["seksjoner"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["heading"] == "Driftsinntekter")
        .unwrap();
    assert_eq!(inntekter["sum_ore"], 1_100_000);

    // An unknown code is a loud error, not an empty report.
    let (status, _) = request(
        &state,
        "GET",
        &format!("/companies/{company}/reports/prosjekt?year=2026&prosjekt=FINNESIKKE"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn dimensions_hash_v3_reports_and_saft() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Dimensjon AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("3000", "Salgsinntekt"),
        ("2700", "Utgående mva"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (_, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunde & Co AS", None, None)
            .await
            .unwrap();
    let token = idp.token(&sub, "Kari Bokfører");

    // Register the dimension registry over the API.
    for (kind, code, name) in [
        ("avdeling", "100", "Oslo"),
        ("avdeling", "200", "Bergen"),
        ("prosjekt", "P42", "Nybygg"),
    ] {
        let (status, body) = request(
            &state,
            "POST",
            &format!("/companies/{company}/dimensions"),
            &token,
            Some(serde_json::json!({ "kind": kind, "code": code, "name": name }).to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "create {kind} {code}: {body}");
    }
    // Duplicate code is rejected; bad kind is rejected.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/dimensions"),
        &token,
        Some(serde_json::json!({ "kind": "avdeling", "code": "100", "name": "X" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/dimensions"),
        &token,
        Some(serde_json::json!({ "kind": "konto", "code": "1", "name": "X" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // An invoice with dims on the revenue line: posts, and the chain
    // (now carrying a v3 voucher) verifies.
    let invoice = |avdeling: Option<&str>, prosjekt: Option<&str>, price: i64| {
        serde_json::json!({
            "party_no": party_no,
            "invoice_date": "2026-03-01",
            "due_date": "2026-03-15",
            "lines": [{
                "description": "Konsulentbistand",
                "unit_price_ore": price,
                "vat_code": "3",
                "avdeling": avdeling,
                "prosjekt": prosjekt,
            }],
        })
        .to_string()
    };
    let (status, issued) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(invoice(Some("100"), Some("P42"), 10_000_00)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    // A dimension-free voucher mixes fine in the same chain.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(invoice(None, None, 5_000_00)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // And one on another avdeling, no prosjekt.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(invoice(Some("200"), None, 2_000_00)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 3, "v3 chain verifies");

    // Unknown dimension code is rejected with a clear message.
    let (status, err) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(invoice(Some("999"), None, 1_000_00)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {err}");

    // Avslutte P42: posting on it is now rejected — like a locked period.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/dimensions/prosjekt/P42"),
        &token,
        Some(serde_json::json!({ "active": false }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, err) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(invoice(None, Some("P42"), 1_000_00)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err.to_string().contains("avsluttet"), "body: {err}");

    // The registry lists all three, P42 closed.
    let (_, dims) = request(
        &state,
        "GET",
        &format!("/companies/{company}/dimensions"),
        &token,
        None,
    )
    .await;
    let p42 = dims["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "P42")
        .unwrap();
    assert_eq!(p42["active"], false);

    // Resultat per dimension: pure SUMs, filtered. Full year revenue is
    // 17 000; avdeling 100 carries 10 000, prosjekt P42 the same line.
    let resultat = |filter: &str| {
        let uri =
            format!("/companies/{company}/reports/resultat?from=2026-01-01&to=2026-12-31{filter}");
        let state = state.clone();
        let token = token.clone();
        async move {
            let (status, body) = request(&state, "GET", &uri, &token, None).await;
            assert_eq!(status, StatusCode::OK, "body: {body}");
            body["arsresultat_ore"].as_i64().unwrap()
        }
    };
    assert_eq!(resultat("").await, 17_000_00);
    assert_eq!(resultat("&avdeling=100").await, 10_000_00);
    assert_eq!(resultat("&avdeling=200").await, 2_000_00);
    assert_eq!(resultat("&prosjekt=P42").await, 10_000_00);
    assert_eq!(resultat("&avdeling=100&prosjekt=P42").await, 10_000_00);
    assert_eq!(resultat("&avdeling=200&prosjekt=P42").await, 0);

    // Kontospesifikasjon carries the dimension columns.
    let (_, spes) = request(
        &state,
        "GET",
        &format!(
            "/companies/{company}/reports/kontospesifikasjon?from=2026-01-01&to=2026-12-31&account=3000"
        ),
        &token,
        None,
    )
    .await;
    let posts = spes["posts"].as_array().unwrap();
    assert!(
        posts
            .iter()
            .any(|p| p["avdeling"] == "100" && p["prosjekt"] == "P42"),
        "{spes}"
    );

    // SAF-T: registry → AnalysisTypeTable, line codes → Analysis.
    let (status, xml_bytes) = send(
        &state,
        "GET",
        &format!("/companies/{company}/reports/saft?year=2026"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let xml = String::from_utf8(xml_bytes).unwrap();
    for expected in [
        "<AnalysisTypeTable>",
        "<AnalysisType>AVD</AnalysisType>",
        "<AnalysisID>100</AnalysisID>",
        "<AnalysisType>PRO</AnalysisType>",
        "<Status>Closed</Status>",
        "<Analysis>",
        "<CreditAnalysisAmount>",
    ] {
        assert!(xml.contains(expected), "missing {expected}");
    }

    // The dimension CODE is immutable — it is inside posted hashes.
    let tampering =
        sqlx::query("update dimension set code = 'X' where company_id = $1 and code = '100'")
            .bind(company)
            .execute(&state.pool)
            .await;
    assert!(tampering.is_err(), "dimension code must be immutable");
    // Rename is allowed (the name is not hashed).
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/dimensions/avdeling/100"),
        &token,
        Some(serde_json::json!({ "name": "Oslo sentrum" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(
        report.vouchers_checked, 3,
        "rename does not break the chain"
    );
}
