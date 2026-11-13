//! Utlegg og kjøregodtgjørelse end to end: immutable receipt from
//! submission, one-way decisions (avvisning requires note), approval
//! posting kostnad + mva mot mellomregning with the receipt attached
//! to the voucher, kjøregodtgjørelse from the dated satsregister with
//! the trekkpliktige del surfaced as a warning, and utbetaling posting
//! mellomregning mot bank. Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
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

async fn balance(state: &AppState, company: Uuid, account: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(account)
    .fetch_one(&state.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn expenses_flow() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Reisende Medarbeider"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Utlegg AS")
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
        ("7790", "Annen kostnad"),
        ("7100", "Bilgodtgjørelse"),
        ("2710", "Inngående mva"),
        ("2910", "Gjeld til ansatte"),
        ("1920", "Bank"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let token = idp.token(&sub, "Reisende Medarbeider");
    let base = format!("/companies/{company}/expenses");

    // Utlegg with a receipt, uploaded as a raw body.
    let receipt = b"kvittering: taxi 625,00 kr inkl. mva".to_vec();
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "{base}/utlegg?filename=taxi.txt&dato=2026-07-01&belop_ore=62500&beskrivelse=Taxi%20kundebes%C3%B8k"
                ))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "text/plain")
                .body(Body::from(receipt.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let made: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let expense_id = made["expense_id"].as_str().unwrap().to_string();

    // The same receipt again is a double submission, not a second claim.
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "{base}/utlegg?filename=taxi-igjen.txt&dato=2026-07-01&belop_ore=62500&beskrivelse=Taxi"
                ))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "text/plain")
                .body(Body::from(receipt.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let message = String::from_utf8_lossy(&bytes);
    assert!(message.contains("allerede sendt inn"), "{message}");
    // A double tap: the second submission of the same receipt waits on
    // the per-company-and-hash advisory lock, then sees the first claim.
    // Held from outside here, so the wait is observed, not hoped for.
    let parking = b"kvittering: parkering 99,00 kr".to_vec();
    let upload = |name: &str| {
        router(state.clone()).oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "{base}/utlegg?filename={name}&dato=2026-07-02&belop_ore=9900&beskrivelse=Parkering"
                ))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "text/plain")
                .body(Body::from(parking.clone()))
                .unwrap(),
        )
    };
    assert_eq!(upload("p1.txt").await.unwrap().status(), StatusCode::OK);
    let mut held = state.pool.begin().await.unwrap();
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text || encode($2, 'hex'), 0))")
        .bind(company)
        .bind(regnmed_core::hash::sha256(&parking).to_vec())
        .execute(&mut *held)
        .await
        .unwrap();
    let second = tokio::spawn(upload("p2.txt"));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !second.is_finished(),
        "the second submission must wait for the lock"
    );
    held.rollback().await.unwrap();
    assert_eq!(
        second.await.unwrap().unwrap().status(),
        StatusCode::BAD_REQUEST
    );

    // A missing amount is a sentence, not a deserializer error.
    let (status, body) = request(
        &state,
        "POST",
        &format!("{base}/kjoring"),
        &token,
        Some(serde_json::json!({ "dato": "2026-07-01", "beskrivelse": "Tur" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.to_string().contains("oppgi antall kilometer"),
        "{body}"
    );

    // Receipt round-trips integrity-checked; content is DB-immutable.
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("{base}/{expense_id}/receipt"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(bytes.to_vec(), receipt);
    let tamper = sqlx::query("update expense set receipt_content = 'forfalsket' where id = $1")
        .bind(Uuid::parse_str(&expense_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "receipt is immutable from submission");
    let tamper = sqlx::query("delete from expense where id = $1")
        .bind(Uuid::parse_str(&expense_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "claims are never deleted");

    // Approve with inngående mva (code 1, 25 %): 625 = 500 + 125.
    let (status, approved) = request(
        &state,
        "POST",
        &format!("{base}/{expense_id}/approve"),
        &token,
        Some(serde_json::json!({ "mva_kode": "1" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {approved}");
    assert!(approved["warning"].is_null());
    assert_eq!(balance(&state, company, "7790").await, 500_00);
    assert_eq!(balance(&state, company, "2710").await, 125_00);
    assert_eq!(balance(&state, company, "2910").await, -625_00);
    // The receipt followed the claim onto the voucher (oppbevaring).
    let attachment_count: i64 = sqlx::query_scalar(
        "select count(*) from attachment a
         join voucher v on v.id = a.voucher_id
         where v.company_id = $1 and a.filename = 'taxi.txt'",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(attachment_count, 1);
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{expense_id}/approve"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "decisions are one-way");

    // The form asks for the satser first, so it can show the amount.
    let (status, rate) = request(
        &state,
        "GET",
        &format!("{base}/kjoring?dato=2026-03-10"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rate}");
    assert_eq!(rate["sats_ore_per_km"], 530);
    assert_eq!(rate["trekkfri_ore_per_km"], 350);

    // Kjøregodtgjørelse: 120 km on 2026-03-10 → 5,30/3,50 kr satser.
    let (status, made) = request(
        &state,
        "POST",
        &format!("{base}/kjoring"),
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-03-10", "beskrivelse": "Oslo–Drammen t/r, kundemøte", "km": 120,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {made}");
    assert_eq!(made["belop_ore"], 63_600);
    assert_eq!(made["trekkpliktig_ore"], 21_600);
    let run_id = made["expense_id"].as_str().unwrap().to_string();
    // Before the register's coverage: refused loudly, never guessed.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/kjoring"),
        &token,
        Some(
            serde_json::json!({ "dato": "2024-03-10", "beskrivelse": "Gammel tur", "km": 10 })
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Approval carries the honest trekkpliktig warning.
    let (status, approved) = request(
        &state,
        "POST",
        &format!("{base}/{run_id}/approve"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        approved["warning"]
            .as_str()
            .unwrap()
            .contains("trekkpliktig del 216,00 kr"),
        "warning: {approved}"
    );
    assert_eq!(balance(&state, company, "7100").await, 636_00);

    // Avvisning requires a note; the note sticks.
    let (status, made) = request(
        &state,
        "POST",
        &format!("{base}/kjoring"),
        &token,
        Some(
            serde_json::json!({ "dato": "2026-07-02", "beskrivelse": "Privat tur", "km": 30 })
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let privat_id = made["expense_id"].as_str().unwrap().to_string();
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{privat_id}/reject"),
        &token,
        Some(serde_json::json!({ "note": "  " }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "tom begrunnelse avvises");
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{privat_id}/reject"),
        &token,
        Some(serde_json::json!({ "note": "privat kjøring dekkes ikke" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Illegal transition straight in the database: trigger says no.
    let tamper = sqlx::query("update expense set status = 'utbetalt' where id = $1")
        .bind(Uuid::parse_str(&privat_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "avvist → utbetalt is not a transition");

    // Utbetaling: mellomregning → bank, one-way; pay of innsendt fails.
    let (status, paid) = request(
        &state,
        "POST",
        &format!("{base}/{expense_id}/pay"),
        &token,
        Some(serde_json::json!({ "dato": "2026-07-05" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {paid}");
    assert_eq!(
        balance(&state, company, "2910").await,
        -636_00,
        "kjøringen står igjen"
    );
    assert_eq!(balance(&state, company, "1920").await, -625_00);
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{expense_id}/pay"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "already paid");

    // The list tells the whole story, and the chain verifies.
    let (_, listed) = request(&state, "GET", &base, &token, None).await;
    let statuses: Vec<String> = listed["expenses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["status"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(statuses.len(), 4);
    assert!(statuses.contains(&"utbetalt".into()));
    assert!(statuses.contains(&"godkjent".into()));
    assert!(statuses.contains(&"avvist".into()));
    assert_eq!(listed["total"], 4);
    assert_eq!(listed["counts"]["utbetalt"], 1);
    assert_eq!(listed["counts"]["godkjent"], 1);
    assert_eq!(listed["counts"]["avvist"], 1);
    assert_eq!(listed["counts"]["innsendt"], 1, "the parking claim");
    // The tabs: «åpne» is what still owes money, the rest are done.
    let (_, open) = request(
        &state,
        "GET",
        &format!("{base}?status=apne&limit=1&offset=5"),
        &token,
        None,
    )
    .await;
    assert_eq!(
        open["total"], 2,
        "a page past the end still knows the total"
    );
    assert!(open["expenses"].as_array().unwrap().is_empty());
    let (_, open) = request(&state, "GET", &format!("{base}?status=apne"), &token, None).await;
    let open_ids: Vec<&str> = open["expenses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["expense_id"].as_str().unwrap())
        .collect();
    assert!(open_ids.contains(&run_id.as_str()), "{open}");
    let (status, _) = request(&state, "GET", &format!("{base}?status=tull"), &token, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // One claim, with both vouchers it produced.
    let (status, one) = request(&state, "GET", &format!("{base}/{expense_id}"), &token, None).await;
    assert_eq!(status, StatusCode::OK, "{one}");
    assert_eq!(one["status"], "utbetalt");
    assert!(
        one["voucher"].is_string() && one["utbetalt_voucher"].is_string(),
        "{one}"
    );
    assert_ne!(one["voucher"], one["utbetalt_voucher"]);
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 3, "utlegg + kjøring + utbetaling");
}

async fn upload(
    state: &AppState,
    uri: &str,
    method: &str,
    bearer: &str,
    bytes: &[u8],
) -> (StatusCode, serde_json::Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "image/jpeg")
                .body(Body::from(bytes.to_vec()))
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

/// Drafts: a receipt saved at once and finished later, invisible to
/// everyone but its claimant and whoever entered it, frozen from
/// submission; dimensions onto the voucher; a trip with route,
/// passengers and its tolls; filing on someone's behalf under four eyes.
#[tokio::test]
async fn expense_drafts_flow() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Utkast AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("7790", "Annen kostnad"),
        ("7100", "Bilgodtgjørelse"),
        ("2910", "Gjeld til ansatte"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::create_dimension(
        &state.pool,
        company,
        "prosjekt",
        "P1",
        "Kundeprosjekt",
        None,
    )
    .await
    .unwrap();
    let mut people = Vec::new();
    for (name, role) in [("Admin Utkast", "admin"), ("Ansatt Utkast", "ansatt")] {
        let sub = format!("test|{}", Uuid::new_v4());
        let id = regnmed_db::ensure_person(&state.pool, &sub, Some(name), None)
            .await
            .unwrap();
        regnmed_db::ensure_company_member(&state.pool, company, id, role)
            .await
            .unwrap();
        people.push((id, idp.token(&sub, name)));
    }
    let (admin, admin_token) = people[0].clone();
    let (employee, employee_token) = people[1].clone();
    let base = format!("/companies/{company}/expenses");

    // A photo alone is a draft: no date, no amount, nothing promised yet.
    let photo = format!("jpeg {}", Uuid::new_v4()).into_bytes();
    let (status, made) = upload(
        &state,
        &format!("{base}/receipts?filename=taxi.jpg"),
        "POST",
        &employee_token,
        &photo,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{made}");
    let draft = made["expense_id"].as_str().unwrap().to_string();
    let (_, got) = request(
        &state,
        "GET",
        &format!("{base}/{draft}"),
        &employee_token,
        None,
    )
    .await;
    assert_eq!(got["status"], "utkast");
    assert_eq!(got["editable"], true);
    assert!(got["dato"].is_null());

    // Drafts stay out of the ordinary lists and are nobody else's business.
    let (_, open) = request(
        &state,
        "GET",
        &format!("{base}?status=apne"),
        &employee_token,
        None,
    )
    .await;
    assert_eq!(open["expenses"].as_array().unwrap().len(), 0);
    let (_, drafts) = request(
        &state,
        "GET",
        &format!("{base}?status=utkast"),
        &employee_token,
        None,
    )
    .await;
    assert_eq!(drafts["counts"]["utkast"], 1);
    let (status, _) = request(
        &state,
        "GET",
        &format!("{base}/{draft}"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "someone else's draft");
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/{draft}"),
        &admin_token,
        Some(serde_json::json!({ "beskrivelse": "kapret" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Incomplete: refused with the missing thing named.
    let (status, body) = request(
        &state,
        "POST",
        &format!("{base}/{draft}/submit"),
        &employee_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.to_string().contains("velg dato"), "{body}");

    let fields = |project: &str| {
        serde_json::json!({
            "dato": "2026-07-03", "belop_ore": 31_200, "beskrivelse": "Taxi til kunde",
            "project": project, "billable": true,
        })
        .to_string()
    };
    let (status, body) = request(
        &state,
        "PUT",
        &format!("{base}/{draft}"),
        &employee_token,
        Some(fields("P9")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.to_string().contains("ukjent prosjekt"), "{body}");
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/{draft}"),
        &employee_token,
        Some(fields("P1")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Rotated or cropped: the picture may still change.
    let cropped = format!("cropped {}", Uuid::new_v4()).into_bytes();
    let (status, _) = upload(
        &state,
        &format!("{base}/{draft}/receipt?filename=taxi.jpg"),
        "PUT",
        &employee_token,
        &cropped,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // ...but not into a picture another claim already has.
    let (status, other) = upload(
        &state,
        &format!("{base}/receipts?filename=b.jpg"),
        "POST",
        &employee_token,
        &photo,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the replaced photo is free again");
    let other = other["expense_id"].as_str().unwrap().to_string();
    let (status, body) = upload(
        &state,
        &format!("{base}/{other}/receipt?filename=c.jpg"),
        "PUT",
        &employee_token,
        &cropped,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.to_string().contains("ligger allerede i et utkast"),
        "{body}"
    );

    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{draft}/submit"),
        &employee_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // From submission the content is evidence — in the API and in the DB.
    let (status, body) = request(
        &state,
        "PUT",
        &format!("{base}/{draft}"),
        &employee_token,
        Some(fields("P1")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.to_string().contains("kan ikke endres"), "{body}");
    let draft_id: Uuid = draft.parse().unwrap();
    for sql in [
        "update expense set project = null where id = $1",
        "update expense set status = 'utkast' where id = $1",
    ] {
        assert!(
            sqlx::query(sql)
                .bind(draft_id)
                .execute(&state.pool)
                .await
                .is_err(),
            "{sql}"
        );
    }

    // Approval puts the project on the cost line, not on the debt.
    let (status, approved) = request(
        &state,
        "POST",
        &format!("{base}/{draft}/approve"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    let tagged: Vec<(String, Option<String>)> = sqlx::query_as(
        "select a.number, d.code from expense x
         join entry e on e.voucher_id = x.voucher_id
         join account a on a.id = e.account_id
         left join dimension d on d.id = e.prosjekt_id
         where x.id = $1 order by a.number",
    )
    .bind(draft_id)
    .fetch_all(&state.pool)
    .await
    .unwrap();
    assert_eq!(
        tagged,
        vec![("2910".into(), None), ("7790".into(), Some("P1".into()))]
    );

    // A trip: round trip, zero-emission car, one colleague, and two
    // tolls — one with its receipt, one still waiting for AutoPASS.
    let (status, rates) = request(
        &state,
        "GET",
        &format!("{base}/kjoring?dato=2026-07-04&vehicle=motorsykkel"),
        &employee_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rates["sats_ore_per_km"], 295);
    assert_eq!(rates["passenger_rate_ore_per_km"], 100);
    let (status, made) = request(
        &state,
        "POST",
        &base,
        &employee_token,
        Some(
            serde_json::json!({
                "kind": "kjoring", "dato": "2026-07-04", "beskrivelse": "Kundemøte",
                "route_from": "Storgata 1, Oslo", "route_to": "Bragernes torg, Drammen",
                "round_trip": true, "km": 100, "vehicle": "bil_utslippsfri",
                "passengers": ["Kari Kollega"],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{made}");
    let trip = made["expense_id"].as_str().unwrap().to_string();
    let mut tolls = Vec::new();
    for what in ["Bompenger Oslo", "Rushtidsavgift"] {
        let (status, made) = request(
            &state,
            "POST",
            &base,
            &employee_token,
            Some(serde_json::json!({
                "trip_expense_id": trip, "dato": "2026-07-04", "beskrivelse": what, "belop_ore": 4_500,
            }).to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{made}");
        tolls.push(made["expense_id"].as_str().unwrap().to_string());
    }
    let toll_receipt = format!("autopass {}", Uuid::new_v4()).into_bytes();
    let (status, _) = upload(
        &state,
        &format!("{base}/{}/receipt?filename=bom.pdf", tolls[0]),
        "PUT",
        &employee_token,
        &toll_receipt,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, done) = request(
        &state,
        "POST",
        &format!("{base}/{trip}/submit"),
        &employee_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    // 100 km × (5,30 + 1,00) kr, of which (3,50 + 1,00) kr is trekkfri.
    assert_eq!(done["belop_ore"], 63_000);
    assert_eq!(done["trekkpliktig_ore"], 18_000);
    assert_eq!(done["costs_submitted"], 1);
    assert_eq!(done["costs_waiting"].as_array().unwrap().len(), 1, "{done}");
    let (_, costs) = request(
        &state,
        "GET",
        &format!("{base}?trip={trip}"),
        &employee_token,
        None,
    )
    .await;
    let statuses: Vec<&str> = costs["expenses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses.len(), 2);
    assert!(statuses.contains(&"innsendt") && statuses.contains(&"utkast"));
    let (_, got) = request(
        &state,
        "GET",
        &format!("{base}/{trip}"),
        &employee_token,
        None,
    )
    .await;
    assert_eq!(got["passenger_rate_ore_per_km"], 100);
    assert_eq!(got["vehicle"], "bil_utslippsfri");
    let (status, approved) = request(
        &state,
        "POST",
        &format!("{base}/{trip}/approve"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    let line: String = sqlx::query_scalar(
        "select e.description from expense x join entry e on e.voucher_id = x.voucher_id
         join account a on a.id = e.account_id where x.id = $1 and a.number = '7100'",
    )
    .bind(trip.parse::<Uuid>().unwrap())
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert!(
        line.contains("Storgata 1, Oslo–Bragernes torg, Drammen t/r")
            && line.contains("Kari Kollega"),
        "{line}"
    );

    // On someone's behalf: only for those who handle claims, only for
    // members, and the one who entered it is no second pair of eyes.
    let stranger = regnmed_db::ensure_person(
        &state.pool,
        &format!("test|{}", Uuid::new_v4()),
        Some("Fremmed"),
        None,
    )
    .await
    .unwrap();
    let on_behalf =
        |who: Uuid| serde_json::json!({ "kind": "kjoring", "person_id": who }).to_string();
    let (status, _) = request(
        &state,
        "POST",
        &base,
        &employee_token,
        Some(on_behalf(admin)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an ansatt files only their own"
    );
    let (status, _) = request(
        &state,
        "POST",
        &base,
        &admin_token,
        Some(on_behalf(stranger)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, made) = request(
        &state,
        "POST",
        &base,
        &admin_token,
        Some(serde_json::json!({
            "kind": "kjoring", "person_id": employee, "dato": "2026-07-05", "beskrivelse": "Lager",
            "route_from": "A", "route_to": "B", "km": 10,
        }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let filed = made["expense_id"].as_str().unwrap().to_string();
    let (_, got) = request(
        &state,
        "GET",
        &format!("{base}/{filed}"),
        &employee_token,
        None,
    )
    .await;
    assert_eq!(
        got["registered_by"], "Admin Utkast",
        "the claimant sees who entered it"
    );
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{filed}/submit"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    regnmed_db::set_policy(&state.pool, company, true, None, None, None, "test")
        .await
        .unwrap();
    let (status, body) = request(
        &state,
        "POST",
        &format!("{base}/{filed}/approve"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.to_string().contains("fire øyne"), "{body}");

    // Discarding a trip takes its drafts along; nothing is deleted.
    let (_, made) = request(
        &state,
        "POST",
        &base,
        &employee_token,
        Some(serde_json::json!({ "kind": "kjoring" }).to_string()),
    )
    .await;
    let trip2 = made["expense_id"].as_str().unwrap().to_string();
    let (_, made) = request(
        &state,
        "POST",
        &base,
        &employee_token,
        Some(serde_json::json!({ "trip_expense_id": trip2 }).to_string()),
    )
    .await;
    let cost2: Uuid = made["expense_id"].as_str().unwrap().parse().unwrap();
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{trip2}/discard"),
        &employee_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let status2: String = sqlx::query_scalar("select status from expense where id = $1")
        .bind(cost2)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(status2, "forkastet");
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/{trip2}/submit"),
        &employee_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "discarded is one way");
}

/// Kartverket and OpenRouteService stood in for by one local server,
/// answering in their documented shapes. Kartverket only knows the
/// street ("Storgata 1 …", "Bragernes torg 1 …"), so a field holding
/// "Company, street, postcode town" is found only by dropping the
/// company — which is what `locate` must do.
async fn start_map_services() -> String {
    use axum::Json;
    use axum::extract::Query;
    use axum::http::HeaderMap;
    use axum::routing::{get, post};
    let app = axum::Router::new()
        .route(
            "/sok",
            get(
                |Query(q): Query<std::collections::HashMap<String, String>>| async move {
                    let text = q.get("sok").cloned().unwrap_or_default();
                    let point = if text.starts_with("Storgata 1") {
                        serde_json::json!({ "lat": 59.9139, "lon": 10.7522 })
                    } else if text.starts_with("Bragernes torg 1") {
                        serde_json::json!({ "lat": 59.7433, "lon": 10.2032 })
                    } else {
                        return Json(serde_json::json!({ "adresser": [] }));
                    };
                    Json(serde_json::json!({ "adresser": [
                        { "adressetekst": text, "representasjonspunkt": point }
                    ]}))
                },
            ),
        )
        .route(
            "/v2/directions/driving-car/geojson",
            post(
                |headers: HeaderMap, Json(body): Json<serde_json::Value>| async move {
                    assert_eq!(headers["authorization"], "test-key");
                    Json(serde_json::json!({ "features": [{
                        "properties": { "summary": { "distance": 41_873.2 } },
                        "geometry": { "coordinates": body["coordinates"] },
                    }]}))
                },
            ),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

#[tokio::test]
async fn a_trip_route_is_found_from_two_addresses() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Rute AS")
        .await
        .unwrap();
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Sjåfør"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "ansatt")
        .await
        .unwrap();
    let token = idp.token(&sub, "Sjåfør");
    let uri = format!(
        "/companies/{company}/expenses/route?from=Nordvik%20AS%2C%20Storgata%201%2C%200155%20OSLO&to=Bragernes%20torg%201%2C%203017%20DRAMMEN"
    );

    // Process-wide env: nextest gives every test its own process.
    unsafe {
        std::env::remove_var("ORS_API_KEY");
    }
    let (status, body) = request(&state, "GET", &uri, &token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["available"], false,
        "no key, no routing — the form asks for km"
    );

    let services = start_map_services().await;
    unsafe {
        std::env::set_var("ORS_API_KEY", "test-key");
        std::env::set_var("ORS_API_URL", &services);
        std::env::set_var("ADDRESS_API_URL", &services);
    }
    let (status, body) = request(&state, "GET", &uri, &token, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["km"], 42);
    assert_eq!(body["from"], serde_json::json!([59.9139, 10.7522]));
    // GeoJSON's [lon, lat] turned into the map's [lat, lon].
    assert_eq!(
        body["line"],
        serde_json::json!([[59.9139, 10.7522], [59.7433, 10.2032]])
    );

    let (status, body) = request(
        &state,
        "GET",
        &format!("/companies/{company}/expenses/route?from=Ingensteds%201&to=Bragernes%20torg%201"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.to_string()
            .contains("fant ikke adressen «Ingensteds 1»"),
        "{body}"
    );
}
