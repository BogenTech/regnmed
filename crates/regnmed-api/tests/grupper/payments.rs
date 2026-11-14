//! Betalingsliste og remittering end to end: open leverandør-poster
//! become an utkast run, approval is a SEPARATE audited action that
//! renders and stores the pain.001 file (hash-checked download),
//! settlement posts the utbetalingsbilag and closes every reskontro-
//! rest in one transaction, and the run history is immutable at the
//! DB layer. Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
use axum::body::Body;
use axum::http::{Request, StatusCode};
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

async fn body_text(state: &AppState, uri: &str, bearer: &str) -> (StatusCode, String) {
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
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn leverandorfaktura(date: chrono::NaiveDate, amount: i64, party_no: &str) -> VoucherDraft {
    VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: format!("Leverandørfaktura {amount}"),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: "4300".into(),
                amount: Ore(amount),
                vat_code: None,
                description: None,
                party_no: None,
                department: None,
                project: None,
                currency: None,
            },
            EntryDraft {
                account_number: "2400".into(),
                amount: Ore(-amount),
                vat_code: None,
                description: Some("Varekjøp".into()),
                party_no: Some(party_no.to_string()),
                department: None,
                project: None,
                currency: None,
            },
        ],
    }
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
async fn betalingsliste_pain001_and_settlement() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Betalings Ansvarlig"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Innkjøp AS")
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
        ("2400", "Leverandørgjeld"),
        ("4300", "Varekostnad"),
        ("1920", "Bank"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_subledger(&state.pool, company, "2400", Some("leverandor"))
        .await
        .unwrap();
    sqlx::query("update company set bank_account = '8601.11.17947' where id = $1")
        .bind(company)
        .execute(&state.pool)
        .await
        .unwrap();
    let (wholesaler_id, wholesaler_no) = regnmed_db::create_party(
        &state.pool,
        company,
        "leverandor",
        "Grossisten AS",
        None,
        None,
    )
    .await
    .unwrap();
    let (_, without_account_no) = regnmed_db::create_party(
        &state.pool,
        company,
        "leverandor",
        "Kontantløs AS",
        None,
        None,
    )
    .await
    .unwrap();

    // Kontonummeret valideres MOD11 før lagring.
    let bad = regnmed_db::update_party_contact(
        &state.pool,
        company,
        wholesaler_id,
        None,
        None,
        Some("86011117948"),
    )
    .await;
    assert!(bad.is_err(), "feil kontrollsiffer avvises");
    regnmed_db::update_party_contact(
        &state.pool,
        company,
        wholesaler_id,
        None,
        None,
        Some("8601 11 17947"),
    )
    .await
    .unwrap();

    let date = |m, d| chrono::NaiveDate::from_ymd_opt(2026, m, d).unwrap();
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &leverandorfaktura(date(7, 1), 12_500_00, &wholesaler_no),
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &leverandorfaktura(date(7, 5), 800_00, &wholesaler_no),
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &leverandorfaktura(date(7, 8), 500_00, &without_account_no),
        "test",
    )
    .await
    .unwrap();
    let token = idp.token(&sub, "Betalings Ansvarlig");
    let base = format!("/companies/{company}/payments");

    // The payable list: three open posts, one flagged without konto.
    let (status, payable) = request(&state, "GET", &format!("{base}/payable"), &token, None).await;
    assert_eq!(status, StatusCode::OK, "body: {payable}");
    let items = payable["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    let entry_of = |amount: i64| {
        items.iter().find(|i| i["belop_ore"] == amount).unwrap()["entry_id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let without_account_entry = items
        .iter()
        .find(|i| i["party_name"] == "Kontantløs AS")
        .unwrap();
    assert!(without_account_entry["bank_account"].is_null());

    // Due date and KID live beside the posting (payable_terms). A due date
    // on the 800 kr post puts it first in the payable list and makes the
    // wholesaler overdue by exactly that post; a bad KID and a post that is
    // not ours are refused.
    let terms = |entry: String, body: serde_json::Value| {
        let state = &state;
        let token = &token;
        async move {
            request(
                state,
                "POST",
                &format!("/companies/{company}/reskontro/entries/{entry}/terms"),
                token,
                Some(body.to_string()),
            )
            .await
        }
    };
    let (status, body) = terms(
        entry_of(800_00),
        serde_json::json!({ "due_date": "2026-07-20" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let (status, _) = terms(entry_of(800_00), serde_json::json!({ "kid": "12345678" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "MOD10/MOD11 is checked");
    let (status, _) = terms(
        Uuid::new_v4().to_string(),
        serde_json::json!({ "kid": "000000026" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, payable) = request(&state, "GET", &format!("{base}/payable"), &token, None).await;
    assert_eq!(
        payable["items"][0]["belop_ore"], 800_00,
        "due first: {payable}"
    );
    assert_eq!(payable["items"][0]["due_date"], "2026-07-20");
    let (_, suppliers) = request(
        &state,
        "GET",
        &format!("/companies/{company}/parties?kind=leverandor&sort=overdue&desc=true&limit=10"),
        &token,
        None,
    )
    .await;
    assert_eq!(suppliers["parties"][0]["party_no"], wholesaler_no.as_str());
    assert_eq!(
        suppliers["parties"][0]["overdue_ore"], 800_00,
        "only the dated post is overdue"
    );
    let (_, wholesaler) = request(
        &state,
        "GET",
        &format!("/companies/{company}/parties/{wholesaler_id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(wholesaler["overdue_ore"], 800_00);

    // A post whose supplier lacks a konto is refused loudly.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/runs"),
        &token,
        Some(
            serde_json::json!({
                "items": [{ "entry_id": without_account_entry["entry_id"] }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The list: full remainder on one post, KID on the other.
    let (status, made) = request(
        &state,
        "POST",
        &format!("{base}/runs"),
        &token,
        Some(
            serde_json::json!({
                "items": [
                    { "entry_id": entry_of(12_500_00), "kid": "000000018" },
                    { "entry_id": entry_of(800_00), "melding": "Faktura 99" },
                ],
                "execution_date": "2026-08-01",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {made}");
    let run_id = made["run_id"].as_str().unwrap().to_string();
    let (status, body) = request(
        &state,
        "POST",
        &format!("{base}/runs"),
        &token,
        Some(serde_json::json!({ "items": [{ "entry_id": entry_of(12_500_00) }] }).to_string()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a post on its way to the bank cannot be listed twice: {body}"
    );
    let (_, payable) = request(&state, "GET", &format!("{base}/payable"), &token, None).await;
    assert!(
        payable["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["i_kjoring"] == true)
            .count()
            == 2,
        "postene er flagget i kjøring"
    );

    // The file exists only after the SEPARATE approval action.
    let (status, _) = body_text(&state, &format!("{base}/runs/{run_id}/file"), &token).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "ingen fil før godkjenning");
    let (status, approved) = request(
        &state,
        "POST",
        &format!("{base}/runs/{run_id}/approve"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {approved}");
    assert!(approved["file_sha256"].is_string());
    let (status, xml) = body_text(&state, &format!("{base}/runs/{run_id}/file"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(xml.contains("pain.001.001.03"));
    assert!(xml.contains("<CtrlSum>13300.00</CtrlSum>"));
    assert!(xml.contains("<Ref>000000018</Ref>"), "KID som SCOR");
    assert!(xml.contains("<Ustrd>Faktura 99</Ustrd>"));
    assert!(
        xml.contains("<Id>86011117947</Id>"),
        "normalisert kontonummer"
    );

    // Run history is evidence at the DB layer.
    let tamper = sqlx::query("update payment_run set execution_date = '2027-01-01' where id = $1")
        .bind(Uuid::parse_str(&run_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "run content is immutable");
    let tamper = sqlx::query("update payment_run_item set belop_ore = 1 where run_id = $1")
        .bind(Uuid::parse_str(&run_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "items are append-only");

    // Settlement: one voucher, every post closed, in one transaction.
    let (status, settled) = request(
        &state,
        "POST",
        &format!("{base}/runs/{run_id}/settle"),
        &token,
        Some(serde_json::json!({ "dato": "2026-08-01" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {settled}");
    assert_eq!(balance(&state, company, "1920").await, -13_300_00);
    assert_eq!(
        balance(&state, company, "2400").await,
        -500_00,
        "bare den kontoløse posten står igjen"
    );
    let open_wholesaler: i64 = sqlx::query_scalar(
        "select count(*) from entry e
         where e.party_id = $1
           and e.amount_ore <> coalesce((select sum(m.amount_ore) from reskontro_match m
                                         where m.entry_a = e.id), 0)
                             + coalesce((select sum(-m.amount_ore) from reskontro_match m
                                         where m.entry_b = e.id), 0)",
    )
    .bind(wholesaler_id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(open_wholesaler, 0, "grossistens reskontro er i null");
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/runs/{run_id}/settle"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "already settled");

    // Cancel is one-way and only from utkast.
    let (_, payable) = request(&state, "GET", &format!("{base}/payable"), &token, None).await;
    assert_eq!(
        payable["items"].as_array().unwrap().len(),
        1,
        "bare den kontoløse posten er åpen"
    );
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/runs/{run_id}/cancel"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "utbetalt kan ikke annulleres"
    );

    // A post with a stored KID is paid WITH it when the list does not
    // name one: the newest terms row applies.
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &leverandorfaktura(date(8, 3), 300_00, &wholesaler_no),
        "test",
    )
    .await
    .unwrap();
    let (_, payable) = request(&state, "GET", &format!("{base}/payable"), &token, None).await;
    let new_post = payable["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["belop_ore"] == 300_00)
        .unwrap()["entry_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = terms(new_post.clone(), serde_json::json!({ "kid": "000000018" })).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = terms(new_post.clone(), serde_json::json!({ "kid": "000000026" })).await;
    assert_eq!(status, StatusCode::OK, "a correction is a new row");
    let (_, made) = request(
        &state,
        "POST",
        &format!("{base}/runs"),
        &token,
        Some(serde_json::json!({ "items": [{ "entry_id": new_post }] }).to_string()),
    )
    .await;
    let second_run = made["run_id"].as_str().unwrap().to_string();
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/runs/{second_run}/approve"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, xml) = body_text(&state, &format!("{base}/runs/{second_run}/file"), &token).await;
    assert!(
        xml.contains("<Ref>000000026</Ref>"),
        "the newest KID: {xml}"
    );
    let rows: i64 = sqlx::query_scalar("select count(*) from payable_terms where entry_id = $1")
        .bind(Uuid::parse_str(&new_post).unwrap())
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(rows, 2, "the corrected KID stays on record");
    let tamper = sqlx::query("update payable_terms set kid = '000000018'")
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "terms are insert-only in the database");

    // The chain verifies over the lot: 4 inngående + utbetalingen.
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 5);
}

/// Approved receipt claims are paid from the same list (docs/utlegg.md
/// «Utbetaling»): to the account in the employee register, whole, and
/// settling the list marks the claim paid and clears 2910. While a claim
/// is on its way to the bank it cannot also be paid by hand.
#[tokio::test]
async fn receipt_claims_are_paid_from_the_payment_list() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Utlegg"), None)
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
        ("2910", "Gjeld til ansatte"),
        ("6800", "Kontorrekvisita"),
        ("1920", "Bank"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    sqlx::query("update company set bank_account = '8601.11.17947' where id = $1")
        .bind(company)
        .execute(&state.pool)
        .await
        .unwrap();
    let date = |m, d| chrono::NaiveDate::from_ymd_opt(2026, m, d).unwrap();
    let claim = regnmed_db::create_expense(
        &state.pool,
        company,
        person,
        date(7, 3),
        "Skrivere",
        349_00,
        "kvittering.txt",
        "text/plain",
        format!("kvittering {}", Uuid::new_v4()).as_bytes(),
        "Kari Utlegg",
    )
    .await
    .unwrap();
    regnmed_db::approve_expense(
        &state.pool,
        company,
        claim,
        "6800",
        None,
        "2710",
        "2910",
        "Kari Utlegg",
        person,
    )
    .await
    .unwrap();
    let token = idp.token(&sub, "Kari Utlegg");
    let base = format!("/companies/{company}/payments");

    // Listed, but without an employee record there is no account to pay.
    let (status, payable) = request(&state, "GET", &format!("{base}/payable"), &token, None).await;
    assert_eq!(status, StatusCode::OK, "body: {payable}");
    let item = |payable: &serde_json::Value| {
        payable["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["expense_id"] == claim.to_string())
            .cloned()
            .unwrap()
    };
    assert!(item(&payable)["bank_account"].is_null());
    assert!(item(&payable)["entry_id"].is_null());
    assert_eq!(item(&payable)["belop_ore"], 349_00);
    let make = |body: serde_json::Value| {
        let state = &state;
        let token = &token;
        let base = &base;
        async move {
            request(
                state,
                "POST",
                &format!("{base}/runs"),
                token,
                Some(body.to_string()),
            )
            .await
        }
    };
    let (status, refused) = make(serde_json::json!({ "items": [{ "expense_id": claim }] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        refused.to_string().contains("ansattregisteret"),
        "{refused}"
    );

    // The employee register holds the account.
    let employee = regnmed_db::payroll::create_employee(
        &state.pool,
        company,
        &regnmed_db::payroll::NewEmployee {
            nin: "26829398612".into(),
            name: "Kari Utlegg".into(),
            stilling: None,
            employed_from: date(1, 1),
            monthly_salary_ore: None,
            hourly_wage_ore: None,
            withholding_type: "prosent".into(),
            withholding_percent_bp: Some(3000),
            withholding_table: None,
            holiday_pay_bp: 1020,
            bank_account: Some("86011117947".into()),
            note: None,
            details: Default::default(),
        },
        "Test",
    )
    .await
    .unwrap();
    sqlx::query("update employee set person_id = $2 where id = $1")
        .bind(employee)
        .bind(person)
        .execute(&state.pool)
        .await
        .unwrap();

    // Paid whole, never in part; neither key or both is refused.
    let (status, _) =
        make(serde_json::json!({ "items": [{ "expense_id": claim, "belop_ore": 100_00 }] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a claim is paid whole");
    let (status, _) = make(serde_json::json!({ "items": [{}] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, made) = make(serde_json::json!({ "items": [{ "expense_id": claim }] })).await;
    assert_eq!(status, StatusCode::OK, "body: {made}");
    let run = made["run_id"].as_str().unwrap().to_string();

    // On its way to the bank: not payable by hand, not in a second list.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/expenses/{claim}/pay"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = make(serde_json::json!({ "items": [{ "expense_id": claim }] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, approved) = request(
        &state,
        "POST",
        &format!("{base}/runs/{run}/approve"),
        &token,
        Some("{}".into()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {approved}");
    let (status, file) = body_text(&state, &format!("{base}/runs/{run}/file"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        file.contains("86011117947"),
        "paid to the employee's account: {file}"
    );
    assert!(file.contains("Kari Utlegg"));

    let (status, settled) = request(
        &state,
        "POST",
        &format!("{base}/runs/{run}/settle"),
        &token,
        Some(serde_json::json!({ "dato": "2026-07-10", "bank_konto": "1920" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {settled}");
    assert_eq!(
        balance(&state, company, "2910").await,
        0,
        "the debt is settled"
    );
    assert_eq!(balance(&state, company, "1920").await, -349_00);
    let status: String = sqlx::query_scalar("select status from expense where id = $1")
        .bind(claim)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(status, "utbetalt");
    let (_, payable) = request(&state, "GET", &format!("{base}/payable"), &token, None).await;
    assert!(
        !payable["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["expense_id"] == claim.to_string()),
        "a paid claim is no longer payable"
    );
}
