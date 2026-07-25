//! Migreringsimport, filtier (#19): kontaktlisten fra det gamle
//! systemet blir parter (idempotent), åpne poster blir ETT bilag med
//! én partslinje per post, reskontrosaldoen blir lik summen av postene
//! fordi det er de samme radene, og en konto som allerede har saldo
//! avvises med tallet i feilmeldingen. Requires DATABASE_URL (skips
//! otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn post_csv(
    state: &AppState,
    uri: &str,
    bearer: &str,
    csv: &str,
) -> (StatusCode, serde_json::Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "text/csv")
                .body(Body::from(csv.to_string()))
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

async fn get_json(state: &AppState, uri: &str, bearer: &str) -> (StatusCode, serde_json::Value) {
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

async fn saldo(state: &AppState, company: Uuid, konto: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(konto)
    .fetch_one(&state.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn kontakter_og_apne_poster_fra_gammelt_system() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Mia Migrering"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Flytter AS")
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
        ("2400", "Leverandørgjeld"),
        ("2050", "Annen egenkapital"),
        ("1920", "Bank"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let token = idp.token(&sub, "Mia Migrering");
    let base = format!("/companies/{company}");

    // ---- Kontakter: Tripletex-aktig eksport ----
    let kunder = "Kundenr;Navn;Organisasjonsnummer;E-post;Adresse;Kontonummer\n\
                  10001;Hansen AS;915933149;post@hansen.no;Storgata 1, 0155 Oslo;8601.11.17947\n\
                  10002;Lille Bakeri;;bakeri@example.no;;\n";
    let (status, body) = post_csv(
        &state,
        &format!("{base}/import/contacts?kind=kunde"),
        &token,
        kunder,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["opprettet"], 2);
    assert_eq!(body["oppdatert"], 0);

    // Kundenummeret følger med — kontinuitet regnskapsføreren ser.
    let (_, parties) = get_json(&state, &format!("{base}/parties?kind=kunde"), &token).await;
    let hansen = parties["parties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Hansen AS")
        .unwrap();
    assert_eq!(hansen["party_no"], "10001");
    assert_eq!(hansen["orgnr"], "915933149");
    assert_eq!(hansen["email"], "post@hansen.no");
    assert_eq!(hansen["bank_account"], "86011117947", "MOD11-validert");

    // Idempotent: samme fil igjen oppdaterer, oppretter ikke på nytt.
    let (_, body) = post_csv(
        &state,
        &format!("{base}/import/contacts?kind=kunde"),
        &token,
        kunder,
    )
    .await;
    assert_eq!(body["opprettet"], 0);
    assert_eq!(body["oppdatert"], 2);

    // ---- Åpne poster: forhåndsvisning før noe bokføres ----
    // Filen har både Beløp og Restbeløp — restbeløpet skal vinne.
    let apne = "Kundenr;Navn;Fakturanr;Fakturadato;Forfallsdato;Beløp;Restbeløp;KID\n\
                10001;Hansen AS;F-100;15.01.2026;29.01.2026;12 500,00;12 500,00;1234567897\n\
                10002;Lille Bakeri;F-101;20.01.2026;03.02.2026;5 000,00;2 000,00;\n\
                10003;Nykommer AS;F-102;22.01.2026;05.02.2026;1 000,00;1 000,00;\n";
    let (status, preview) = post_csv(
        &state,
        &format!("{base}/import/open-items?kind=kunde&preview=true"),
        &token,
        apne,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["antall"], 3);
    assert_eq!(preview["sum_ore"], 15_500_00, "12 500 + 2 000 + 1 000");
    assert_eq!(preview["konto_saldo_ore"], 0);
    assert_eq!(preview["kan_importeres"], true);
    assert_eq!(
        preview["nye_parter"].as_array().unwrap(),
        &vec![serde_json::json!("Nykommer AS")],
        "bare den ukjente parten"
    );
    assert_eq!(
        saldo(&state, company, "1500").await,
        0,
        "intet bokført ennå"
    );

    // ---- Åpne poster: import ----
    let (status, body) = post_csv(
        &state,
        &format!("{base}/import/open-items?kind=kunde&dato=2026-01-01"),
        &token,
        apne,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["antall"], 3);
    assert_eq!(body["sum_ore"], 15_500_00);
    assert_eq!(body["opprettede_parter"], 1);

    // Reskontroen ER hovedboken her: saldoen er summen av postene fordi
    // det er de samme radene.
    assert_eq!(saldo(&state, company, "1500").await, 15_500_00);
    assert_eq!(saldo(&state, company, "2050").await, -15_500_00);

    // Hver post ligger åpen på sin part, med fakturanummeret i teksten.
    let (_, parties) = get_json(&state, &format!("{base}/parties?kind=kunde"), &token).await;
    let hansen_id = parties["parties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Hansen AS")
        .unwrap()["party_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, items) =
        get_json(&state, &format!("{base}/parties/{hansen_id}/items"), &token).await;
    assert_eq!(status, StatusCode::OK, "{items}");
    let open: Vec<_> = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["remaining_ore"] != 0)
        .collect();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0]["remaining_ore"], 12_500_00);
    assert!(
        open[0]["description"]
            .as_str()
            .unwrap_or_default()
            .contains("F-100"),
        "fakturanummeret følger posten: {}",
        open[0]["description"]
    );

    // ---- En konto med saldo avvises, med tallet i meldingen ----
    let (status, body) = post_csv(
        &state,
        &format!("{base}/import/open-items?kind=kunde"),
        &token,
        apne,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error = body["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("1550000"),
        "saldoen står i meldingen: {error}"
    );
    assert!(error.contains("ERSTATTER"), "{error}");

    // ---- Leverandørposter havner på kreditsiden ----
    let leverandorer = "Leverandørnr;Navn;Bilagsnr;Dato;Saldo\n\
                        20001;Grossisten AS;I-77;2026-02-01;4 500,00\n";
    let (status, body) = post_csv(
        &state,
        &format!("{base}/import/contacts?kind=leverandor"),
        &token,
        "Leverandørnr;Navn\n20001;Grossisten AS\n",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = post_csv(
        &state,
        &format!("{base}/import/open-items?kind=leverandor&dato=2026-02-01"),
        &token,
        leverandorer,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        saldo(&state, company, "2400").await,
        -4_500_00,
        "leverandørgjeld er kredit"
    );

    // ---- En fil vi ikke forstår feiler høyt med kolonnene ----
    let (status, body) = post_csv(
        &state,
        &format!("{base}/import/open-items?kind=kunde"),
        &token,
        "Kolonne A;Kolonne B\n1;2\n",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error = body["error"].as_str().unwrap_or_default();
    assert!(error.contains("kolonne a"), "viser hva vi leste: {error}");
}
