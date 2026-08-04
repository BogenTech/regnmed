//! Timeføring end to end: integer minutes recorded/edited/deleted, the
//! month lock rejects changes (also at the trigger layer) while billing
//! locked hours stays possible, and the fakturagrunnlag becomes an
//! ordinary invoice with the prosjekt dimension — hours marked
//! fakturert one-way. Requires DATABASE_URL (skips otherwise).

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

#[tokio::test]
async fn hours_lock_and_bill() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Konsulent"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Timer AS")
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
    let (_, party_no) = regnmed_db::create_party(
        &state.pool,
        company,
        "kunde",
        "Oppdragsgiver AS",
        None,
        None,
    )
    .await
    .unwrap();
    // P1 is linked to its customer (#80) — the fakturagrunnlag suggests
    // the recipient from this link further down.
    regnmed_db::create_dimension(
        &state.pool,
        company,
        "prosjekt",
        "P1",
        "Leveranse",
        Some(&party_no),
    )
    .await
    .unwrap();
    let token = idp.token(&sub, "Kari Konsulent");
    let base = format!("/companies/{company}/timesheet");

    // Record 2,5 h billable on P1 (150 min) + 1 h internal.
    let (status, first) = request(
        &state,
        "POST",
        &base,
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-06",
                "minutter": 150,
                "beskrivelse": "Implementasjon",
                "prosjekt": "P1",
                "fakturerbar": true,
                "timesats_ore": 1_200_00,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {first}");
    let entry_id = first["entry_id"].as_str().unwrap().to_string();
    let (status, _) = request(
        &state,
        "POST",
        &base,
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-07",
                "minutter": 60,
                "beskrivelse": "Internmøte",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Billable without sats is rejected; unknown prosjekt is rejected.
    let (status, _) = request(
        &state,
        "POST",
        &base,
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-07", "minutter": 30, "beskrivelse": "X", "fakturerbar": true,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = request(
        &state,
        "POST",
        &base,
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-07", "minutter": 30, "beskrivelse": "X", "prosjekt": "P99",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Edit while open: 150 → 180 min.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/{entry_id}"),
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-06",
                "minutter": 180,
                "beskrivelse": "Implementasjon",
                "prosjekt": "P1",
                "fakturerbar": true,
                "timesats_ore": 1_200_00,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Weekly view + summary.
    let (_, week) = request(
        &state,
        "GET",
        &format!("{base}?from=2026-07-06&to=2026-07-12"),
        &token,
        None,
    )
    .await;
    assert_eq!(week["entries"].as_array().unwrap().len(), 2);
    let (_, summary) = request(
        &state,
        "GET",
        &format!("{base}/summary?from=2026-07-01&to=2026-07-31"),
        &token,
        None,
    )
    .await;
    let p1 = summary["prosjekter"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["prosjekt"] == "P1")
        .unwrap();
    assert_eq!(p1["minutter"], 180);
    assert_eq!(p1["ufakturert_ore"], 3_600_00, "3 t à 1200 kr");

    // Lock July (admin): edits and inserts in July now fail — including
    // straight at the database (the trigger, not just the API).
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/lock"),
        &token,
        Some(serde_json::json!({ "locked_through": "2026-07-31" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/{entry_id}"),
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-06", "minutter": 60, "beskrivelse": "krymp",
                "prosjekt": "P1", "fakturerbar": true, "timesats_ore": 1_200_00,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "locked month rejects edits"
    );
    let direct = sqlx::query("delete from time_entry where id = $1")
        .bind(Uuid::parse_str(&entry_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(direct.is_err(), "trigger guards the lock at the DB layer");

    // Billing LOCKED hours is still allowed: fakturagrunnlag → invoice
    // with the prosjekt dimension, entries marked fakturert.
    let (_, unbilled) = request(&state, "GET", &format!("{base}/unbilled"), &token, None).await;
    assert_eq!(unbilled["groups"].as_array().unwrap().len(), 1);
    assert_eq!(unbilled["groups"][0]["minutter"], 180);
    // The group carries the project's customer — the SUGGESTED
    // recipient (#80). Billing below still names the party explicitly.
    assert_eq!(unbilled["groups"][0]["kunde"], party_no.as_str());
    assert_eq!(unbilled["groups"][0]["kunde_navn"], "Oppdragsgiver AS");
    let (status, issued) = request(
        &state,
        "POST",
        &format!("{base}/invoice"),
        &token,
        Some(serde_json::json!({ "party_no": party_no }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    assert_eq!(issued["gross_ore"], 4_500_00, "3 t à 1200 + 25 % mva");
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 1);
    // The revenue entry carries the prosjekt dimension.
    let dim_code: Option<String> = sqlx::query_scalar(
        "select d.code from entry e
         join dimension d on d.id = e.prosjekt_id
         join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = '3000'",
    )
    .bind(company)
    .fetch_optional(&state.pool)
    .await
    .unwrap();
    assert_eq!(dim_code.as_deref(), Some("P1"));

    // Fakturerte timer are immutable and never rebilled.
    let (_, again) = request(&state, "GET", &format!("{base}/unbilled"), &token, None).await;
    assert!(again["groups"].as_array().unwrap().is_empty());
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/invoice"),
        &token,
        Some(serde_json::json!({ "party_no": party_no }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "nothing left to bill");
    let tamper = sqlx::query("update time_entry set minutter = 1 where id = $1")
        .bind(Uuid::parse_str(&entry_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(
        tamper.is_err(),
        "billed hours are immutable at the DB layer"
    );

    // The week view links the hours to their invoice.
    let (_, week) = request(
        &state,
        "GET",
        &format!("{base}?from=2026-07-06&to=2026-07-12"),
        &token,
        None,
    )
    .await;
    let billed = week["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["entry_id"] == entry_id.as_str())
        .unwrap();
    assert_eq!(billed["invoice_no"], 1);
}

/// The timer rettigheter mean what their descriptions say (#38 follow-up):
/// an ansatt sees ONLY their own hours, `TIMER_LES_ALLE` (bokforing,
/// revisor, admin) unlocks the whole team, correcting someone else's entry
/// requires `TIMER_SKRIV_ALLE` — also through a custom role, not only the
/// admin role — and billing requires `TIMER_FAKTURER`, which an ansatt
/// does not hold.
///
/// Auth-test lesson from docs/auth.md applies: every probe here reaches
/// the guard with a valid body and existing rows, so a refusal measures
/// the guard and nothing else.
#[tokio::test]
async fn timer_rights_mean_what_they_say() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let admin_sub = format!("test|{}", Uuid::new_v4());
    let ansatt_sub = format!("test|{}", Uuid::new_v4());
    let bokforer_sub = format!("test|{}", Uuid::new_v4());
    let timesjef_sub = format!("test|{}", Uuid::new_v4());
    let admin = regnmed_db::ensure_person(&state.pool, &admin_sub, Some("Astrid Admin"), None)
        .await
        .unwrap();
    let ansatt = regnmed_db::ensure_person(&state.pool, &ansatt_sub, Some("Espen Ansatt"), None)
        .await
        .unwrap();
    let bokforer = regnmed_db::ensure_person(&state.pool, &bokforer_sub, Some("Berit Bok"), None)
        .await
        .unwrap();
    let timesjef = regnmed_db::ensure_person(&state.pool, &timesjef_sub, Some("Trine Sjef"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Timevakt AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, admin, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, ansatt, "ansatt")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, bokforer, "bokforing")
        .await
        .unwrap();
    // A company-defined role carrying exactly the two ALLE-rights: the
    // enforcement must read the rettighet, not the role name.
    regnmed_db::roller::opprett(
        &state.pool,
        company,
        "Timesjef",
        &["TIMER_LES_ALLE".to_string(), "TIMER_SKRIV_ALLE".to_string()],
        admin,
        "Astrid Admin",
    )
    .await
    .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, timesjef, "Timesjef")
        .await
        .unwrap();
    // Ledger scaffolding for the billing probe.
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
        regnmed_db::create_party(&state.pool, company, "kunde", "Kjøper AS", None, None)
            .await
            .unwrap();

    let admin_token = idp.token(&admin_sub, "Astrid Admin");
    let ansatt_token = idp.token(&ansatt_sub, "Espen Ansatt");
    let bokforer_token = idp.token(&bokforer_sub, "Berit Bok");
    let timesjef_token = idp.token(&timesjef_sub, "Trine Sjef");
    let base = format!("/companies/{company}/timesheet");

    // One entry each: the admin's own, and the ansatt's own billable hour.
    let (status, _) = request(
        &state,
        "POST",
        &base,
        &admin_token,
        Some(
            serde_json::json!({
                "dato": "2026-08-03", "minutter": 60, "beskrivelse": "Styremøte",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, created) = request(
        &state,
        "POST",
        &base,
        &ansatt_token,
        Some(
            serde_json::json!({
                "dato": "2026-08-03", "minutter": 60, "beskrivelse": "Levering",
                "fakturerbar": true, "timesats_ore": 1_000_00,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {created}");
    let ansatt_entry = created["entry_id"].as_str().unwrap().to_string();

    // Visibility: the ansatt sees only their own row; TIMER_LES_ALLE
    // (bokforing, custom role, admin) sees both.
    let uke = format!("{base}?from=2026-08-03&to=2026-08-09");
    let (_, sett) = request(&state, "GET", &uke, &ansatt_token, None).await;
    let egne = sett["entries"].as_array().unwrap();
    assert_eq!(egne.len(), 1, "ansatt ser bare egne timer: {sett}");
    assert_eq!(egne[0]["own"], true);
    for token in [&bokforer_token, &timesjef_token, &admin_token] {
        let (_, alle) = request(&state, "GET", &uke, token, None).await;
        assert_eq!(
            alle["entries"].as_array().unwrap().len(),
            2,
            "TIMER_LES_ALLE ser hele laget: {alle}"
        );
    }

    // Correcting someone else's entry follows TIMER_SKRIV_ALLE: the
    // bokforing role does not hold it, the custom role does.
    let korreksjon = serde_json::json!({
        "dato": "2026-08-03", "minutter": 90, "beskrivelse": "Levering",
        "fakturerbar": true, "timesats_ore": 1_000_00,
    })
    .to_string();
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/{ansatt_entry}"),
        &bokforer_token,
        Some(korreksjon.clone()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "bokforing retter ikke andres timer"
    );
    let (status, body) = request(
        &state,
        "PUT",
        &format!("{base}/{ansatt_entry}"),
        &timesjef_token,
        Some(korreksjon),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "TIMER_SKRIV_ALLE retter: {body}");

    // Billing requires TIMER_FAKTURER: the ansatt is refused even with
    // billable hours on the table, the bokfører goes through.
    let bill = serde_json::json!({ "party_no": party_no }).to_string();
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/invoice"),
        &ansatt_token,
        Some(bill.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "ansatt fakturerer ikke");
    let (status, issued) = request(
        &state,
        "POST",
        &format!("{base}/invoice"),
        &bokforer_token,
        Some(bill),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
}

/// Billing a SELECTION of the grunnlag: the fakturagrunnlag names who
/// the hours belong to, an invoice can take one person's hours and
/// leave the rest unbilled — and the selection is locked by the invoice
/// itself (marked fakturert in the same transaction), so a stale or
/// already-billed selection fails whole rather than billing less than
/// what was chosen.
#[tokio::test]
async fn a_selection_of_the_grunnlag_bills_and_locks_exactly_itself() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let a_sub = format!("test|{}", Uuid::new_v4());
    let b_sub = format!("test|{}", Uuid::new_v4());
    let a = regnmed_db::ensure_person(&state.pool, &a_sub, Some("Anna"), None)
        .await
        .unwrap();
    let b = regnmed_db::ensure_person(&state.pool, &b_sub, Some("Bjørn"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Utvalg AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, a, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, b, "ansatt")
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
        regnmed_db::create_party(&state.pool, company, "kunde", "Kjøper AS", None, None)
            .await
            .unwrap();
    let a_token = idp.token(&a_sub, "Anna");
    let b_token = idp.token(&b_sub, "Bjørn");
    let base = format!("/companies/{company}/timesheet");

    // Two hours Anna, one hour Bjørn — same (prosjekt, sats) group.
    let timer = |dato: &str, minutter: i32| {
        serde_json::json!({
            "dato": dato, "minutter": minutter, "beskrivelse": "Arbeid",
            "fakturerbar": true, "timesats_ore": 1_000_00,
        })
        .to_string()
    };
    let (status, _) = request(
        &state,
        "POST",
        &base,
        &a_token,
        Some(timer("2026-08-03", 120)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, bjorns) = request(
        &state,
        "POST",
        &base,
        &b_token,
        Some(timer("2026-08-04", 60)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let bjorn_entry = bjorns["entry_id"].as_str().unwrap().to_string();

    // The grunnlag names both, with each person's entry ids.
    let (_, unbilled) = request(&state, "GET", &format!("{base}/unbilled"), &a_token, None).await;
    let personer = unbilled["groups"][0]["personer"].as_array().unwrap();
    assert_eq!(personer.len(), 2, "{unbilled}");
    let bjorn_ids: Vec<String> =
        personer.iter().find(|p| p["navn"] == "Bjørn").unwrap()["entry_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
    assert_eq!(bjorn_ids, vec![bjorn_entry.clone()]);

    // Bill ONLY Bjørn's hour: 1 t à 1000 kr + 25 % mva.
    let (status, issued) = request(
        &state,
        "POST",
        &format!("{base}/invoice"),
        &a_token,
        Some(serde_json::json!({ "party_no": party_no, "entry_ids": bjorn_ids }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    assert_eq!(issued["gross_ore"], 1_250_00);

    // Bjørn's hour is locked by the invoice (DB trigger), Anna's remains
    // in the grunnlag.
    let tamper = sqlx::query("update time_entry set minutter = 1 where id = $1")
        .bind(Uuid::parse_str(&bjorn_entry).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "valgte og fakturerte timer er låst");
    let (_, rest) = request(&state, "GET", &format!("{base}/unbilled"), &a_token, None).await;
    assert_eq!(rest["groups"][0]["minutter"], 120, "{rest}");

    // A stale selection (already billed) fails whole.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/invoice"),
        &a_token,
        Some(serde_json::json!({ "party_no": party_no, "entry_ids": [bjorn_entry] }).to_string()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "gammelt utvalg feiler helt"
    );
}
