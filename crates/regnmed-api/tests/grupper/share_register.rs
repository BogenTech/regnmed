//! Aksjeeierbok and aksjonærregisteroppgave (#43): the holding is
//! computed and never stored, the events cannot be changed, the dividend
//! is posted in the same transaction as the decision, and the oppgave
//! that comes out validates against Skatteetaten's own XSDs.
//!
//! Requires DATABASE_URL — skips politely otherwise.

use crate::common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn call(
    state: &AppState,
    token: &str,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    let body = match body {
        Some(v) => {
            request = request.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let response = router(state.clone())
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, value)
}

/// Validates against a vendored XSD; skips when xmllint is missing.
fn valider(xml: &str, xsd_name: &str, tag: &str) {
    let xsd = format!(
        "{}/../../docs/aksjonaer/{xsd_name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let dir = std::env::temp_dir().join("regnmed-aksjonaer-api-test");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{tag}.xml"));
    std::fs::write(&file, xml).unwrap();
    let Ok(output) = std::process::Command::new("xmllint")
        .args(["--noout", "--schema", &xsd])
        .arg(&file)
        .output()
    else {
        eprintln!("xmllint ikke installert — hopper over skjemavalidering");
        return;
    };
    assert!(
        output.status.success(),
        "XSD-validering feilet for {tag}:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn share_register_dividends_and_the_return() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Styreleder"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Aksjeselskapet AS")
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
        ("2050", "Annen egenkapital"),
        ("2800", "Avsatt utbytte"),
        ("1920", "Bankinnskudd"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let token = idp.token(&sub, "Kari Styreleder");
    let base = format!("/companies/{company}");

    // Two shareholders: a person and a company.
    let (status, body) = call(
        &state,
        &token,
        "POST",
        &format!("{base}/shareholders"),
        Some(json!({
            "kind": "person",
            "navn": "Kari Nordmann",
            "fodselsnummer": "26829398612",
            "adresse": "Haråsveien 13E",
            "postnummer": "0283",
            "poststed": "OSLO"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let kari: Uuid = serde_json::from_value(body["shareholder_id"].clone()).unwrap();

    let (status, body) = call(
        &state,
        &token,
        "POST",
        &format!("{base}/shareholders"),
        Some(json!({
            "kind": "selskap",
            "navn": "Investor AS",
            "orgnr": "923609016"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let investor: Uuid = serde_json::from_value(body["shareholder_id"].clone()).unwrap();

    // A fødselsnummer with a broken check digit is a typo we catch now,
    // not in January.
    let (status, body) = call(
        &state,
        &token,
        "POST",
        &format!("{base}/shareholders"),
        Some(json!({
            "kind": "person", "navn": "Feil Nummer", "fodselsnummer": "26829398613"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("fødselsnummer"));

    // Stiftelse: 100 shares to Kari, nominal value 1000 kr.
    let (status, body) = call(
        &state,
        &token,
        "POST",
        &format!("{base}/share-events"),
        Some(json!({
            "shareholder_id": kari, "type": "stiftelse", "dato": "2025-01-02",
            "antall": 100, "belop_ore": 10_000_000i64
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Transfer in 2026: Kari sells 40 to Investor. Two rows, one tx.
    let (status, body) = call(
        &state,
        &token,
        "POST",
        &format!("{base}/share-events"),
        Some(json!({
            "shareholder_id": kari, "type": "salg", "dato": "2026-06-01",
            "antall": 40, "belop_ore": 4_000_000i64,
            "motpart_id": investor, "motpart_type": "kjop"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The aksjeeierbok is computed — and it is a function of the date.
    let (_, body) = call(
        &state,
        &token,
        "GET",
        &format!("{base}/shareholders?dato=2025-06-01"),
        None,
    )
    .await;
    assert_eq!(body["totalt_antall_aksjer"], 100);
    let kari_2025 = body["aksjonarer"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == json!(kari))
        .unwrap()
        .clone();
    assert_eq!(kari_2025["antall_aksjer"], 100);
    assert_eq!(kari_2025["andel_bp"], 10_000);
    // §4-5 asks for the birth date — the number must not be in the listing.
    assert_eq!(kari_2025["fodselsdato"], "1993-02-26");
    assert!(
        !body.to_string().contains("26829398612"),
        "fødselsnummeret skal ikke ligge i aksjeeierbok-visningen"
    );

    let (_, body) = call(
        &state,
        &token,
        "GET",
        &format!("{base}/shareholders?dato=2026-12-31"),
        None,
    )
    .await;
    assert_eq!(body["totalt_antall_aksjer"], 100);
    for a in body["aksjonarer"].as_array().unwrap() {
        let forventet = if a["id"] == json!(kari) { 60 } else { 40 };
        assert_eq!(a["antall_aksjer"], forventet, "{a}");
    }

    // Nobody can sell more shares than they own.
    let (status, body) = call(
        &state,
        &token,
        "POST",
        &format!("{base}/share-events"),
        Some(json!({
            "shareholder_id": investor, "type": "salg", "dato": "2026-07-01", "antall": 999
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("kan ikke avhende"));

    // Dividend: the resolution is posted in the same transaction.
    let (status, body) = call(
        &state,
        &token,
        "POST",
        &format!("{base}/dividends"),
        Some(json!({ "besluttet_dato": "2026-05-20", "per_aksje_ore": 50_000i64 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // 100 aksjer x 500 kr = 50 000 kr.
    assert_eq!(body["totalt_ore"], 5_000_000i64);
    assert!(body["voucher_id"].is_string(), "utbyttet skal ha et bilag");

    // Oppgaven: forhåndsvisning først.
    let (status, body) = call(
        &state,
        &token,
        "GET",
        &format!("{base}/reports/aksjonaeroppgave?year=2026"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["antall_aksjonarer"], 2);
    assert_eq!(body["antall_aksjer"], 100);
    assert_eq!(body["antall_aksjer_fjoraret"], 100);
    assert_eq!(body["palydende_ore"], 100_000i64);
    // A sale is fileable now: K for the buyer, R for the seller
    // (Skatteetaten's design and example files, docs/aksjonaer.md).
    assert_eq!(body["leverbar"], true, "{body}");

    // A transfer with continuity names the other party on both forms —
    // the portal takes the other side's type from the type list.
    let (_, typer) = call(
        &state,
        &token,
        "GET",
        &format!("{base}/shareholders/transaction-types"),
        None,
    )
    .await;
    let ok = typer["typer"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["slug"] == "overforing_med_kontinuitet_avgang")
        .unwrap()
        .clone();
    assert_eq!(ok["motpart_type"], "overforing_med_kontinuitet");
    assert_eq!(ok["leverbar"], true);
    let (status, body) = call(
        &state,
        &token,
        "POST",
        &format!("{base}/share-events"),
        Some(json!({
            "shareholder_id": kari, "type": "overforing_med_kontinuitet_avgang",
            "dato": "2026-09-01", "antall": 10,
            "motpart_id": investor, "motpart_type": ok["motpart_type"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = call(
        &state,
        &token,
        "GET",
        &format!("{base}/reports/aksjonaeroppgave?year=2026&format=xml"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let under: Vec<&str> = body["underskjemaer"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["xml"].as_str().unwrap())
        .collect();
    let kari_form = under
        .iter()
        .find(|x| x.contains("orid=\"1156\">26829398612<"))
        .unwrap();
    let investor_form = under
        .iter()
        .find(|x| x.contains("AksjonarOrganisasjonsnummer"))
        .unwrap();
    assert!(kari_form.contains("orid=\"17753\">R<"), "{kari_form}");
    assert!(kari_form.contains("orid=\"17753\">OK<"), "{kari_form}");
    assert!(
        kari_form.contains("orid=\"26533\">923609016<"),
        "{kari_form}"
    );
    assert!(
        investor_form.contains("orid=\"17745\">K<"),
        "{investor_form}"
    );
    assert!(
        investor_form.contains("orid=\"17745\">OK<"),
        "{investor_form}"
    );
    assert!(
        investor_form.contains("orid=\"26530\">26829398612<"),
        "{investor_form}"
    );
    for (i, xml) in under.iter().enumerate() {
        valider(
            xml,
            "aksjonaerregisteroppgaveUnderskjema.xsd",
            &format!("api-2026-{i}"),
        );
    }

    // The stiftelse year has only codes we HAVE verified, and is filed.
    let (status, body) = call(
        &state,
        &token,
        "GET",
        &format!("{base}/reports/aksjonaeroppgave?year=2025&format=xml"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let hoved = body["hovedskjema"].as_str().unwrap();
    assert!(hoved.contains("blankettnummer=\"RF-1086\""));
    assert!(hoved.contains("<Inntektsar-datadef-692 orid=\"692\">2025</Inntektsar-datadef-692>"));
    valider(
        hoved,
        "aksjonaerregisteroppgaveHovedskjema.xsd",
        "api-hoved",
    );

    // Only Kari: Investor AS bought in 2026 and was no shareholder in 2025,
    // so it gets no underskjema for that year.
    let under = body["underskjemaer"].as_array().unwrap();
    assert_eq!(under.len(), 1);
    for (i, u) in under.iter().enumerate() {
        let xml = u["xml"].as_str().unwrap();
        assert!(xml.contains("blankettnummer=\"RF-1086-U\""));
        valider(
            xml,
            "aksjonaerregisteroppgaveUnderskjema.xsd",
            &format!("api-under-{i}"),
        );
    }
    // The fødselsnummer belongs HERE, in the filing — and only here.
    assert!(
        under
            .iter()
            .any(|u| u["xml"].as_str().unwrap().contains("26829398612")),
        "innsendingen må identifisere den personlige aksjonæren"
    );
}

#[tokio::test]
async fn events_cannot_be_changed_or_deleted() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Ola"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Uforanderlig AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();

    let holder = regnmed_db::share_register::create_shareholder(
        &state.pool,
        company,
        &regnmed_db::share_register::NewShareholder {
            kind: "selskap".into(),
            name: "Eier AS".into(),
            nin: None,
            orgnumber: Some("923609016".into()),
            foreign_id: None,
            address: None,
            postal_code: None,
            postal_city: None,
            country_code: None,
            note: None,
        },
        "Ola",
    )
    .await
    .unwrap();
    let event = regnmed_db::share_register::record_hendelse(
        &state.pool,
        company,
        holder,
        "stiftelse",
        chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
        30,
        Some(3_000_000),
        None,
        None,
        None,
        "Ola",
    )
    .await
    .unwrap();

    // The events are insert-only, enforced by the database itself.
    let err = sqlx::query("update share_event set antall = 999 where id = $1")
        .bind(event)
        .execute(&state.pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("innsettings-bare"), "{err}");

    let err = sqlx::query("delete from share_event where id = $1")
        .bind(event)
        .execute(&state.pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("innsettings-bare"), "{err}");

    // A shareholder's identity is not editable either…
    let err = sqlx::query("update shareholder set orgnr = '974760673' where id = $1")
        .bind(holder)
        .execute(&state.pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("uforanderlig"), "{err}");

    // …but the address is, because people move.
    regnmed_db::share_register::update_shareholder_contact(
        &state.pool,
        company,
        holder,
        "Eier AS",
        Some("Ny gate 1"),
        Some("0150"),
        Some("OSLO"),
        None,
    )
    .await
    .unwrap();
}

/// Skatteetatens RF-1086 API in a box: records (path, idempotencyKey) per
/// call and refuses the first `fail_confirms` bekreft calls.
async fn mock_skatteetaten(
    fail_confirms: usize,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
) {
    use axum::extract::{Path, State};
    use axum::http::HeaderMap;
    use axum::routing::post;
    use std::sync::{Arc, Mutex};
    type Calls = Arc<Mutex<Vec<(String, String)>>>;
    fn record(calls: &Calls, path: String, headers: &HeaderMap) -> usize {
        let key = headers["idempotencyKey"].to_str().unwrap().to_string();
        let mut c = calls.lock().unwrap();
        c.push((path, key));
        c.iter().filter(|(p, _)| p.ends_with("bekreft")).count()
    }
    let calls: Calls = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new()
        .route(
            "/token",
            post(|| async { axum::Json(json!({"access_token": "t", "expires_in": 120})) }),
        )
        .route(
            "/v1/{year}/1086H",
            post(
                |State(c): State<Calls>, Path(year): Path<String>, h: HeaderMap| async move {
                    record(&c, format!("{year}/1086H"), &h);
                    axum::Json(json!({"hovedskjemaId": "h-1"}))
                },
            ),
        )
        .route(
            "/v1/{year}/{main}/1086U",
            post(
                |State(c): State<Calls>,
                 Path((year, main)): Path<(String, String)>,
                 h: HeaderMap| async move {
                    record(&c, format!("{year}/{main}/1086U"), &h);
                },
            ),
        )
        .route(
            "/v1/{year}/{main}/bekreft",
            post(
                move |State(c): State<Calls>,
                      Path((year, main)): Path<(String, String)>,
                      h: HeaderMap| async move {
                    if record(&c, format!("{year}/{main}/bekreft"), &h) <= fail_confirms {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            axum::Json(json!({"kode": "GLD_023", "melding": "prøv igjen"})),
                        );
                    }
                    (
                        StatusCode::OK,
                        axum::Json(json!({
                            "oppgavegiversLeveranseReferanse": "h-1",
                            "dialogId": "d-1",
                            "forsendelseId": "f-1"
                        })),
                    )
                },
            ),
        )
        .route(
            "/dp/enduser/dialogs/{id}",
            axum::routing::get(|| async {
                axum::Json(json!({"transmissions": [
                    {"id": "f-1", "type": "Submission"},
                    {"id": "a-1", "type": "Acceptance", "relatedTransmissionId": "f-1",
                     "content": {"title": {"value": [{"languageCode": "nb",
                        "value": "Innsending av godkjent Aksjonærregisteroppgave"}]}},
                     "attachments": [{"displayName": [{"languageCode": "nb", "value": "tilbakemelding.pdf"}],
                                      "urls": [{"consumerType": "Gui", "url": "https://x/t.pdf"}]}]}
                ]}))
            }),
        )
        .with_state(calls.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, calls)
}

async fn add_shareholder(
    state: &AppState,
    token: &str,
    base: &str,
    navn: &str,
    nin: &str,
) -> serde_json::Value {
    let (status, body) = call(
        state,
        token,
        "POST",
        &format!("{base}/shareholders"),
        Some(json!({"kind": "person", "navn": navn, "fodselsnummer": nin})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["shareholder_id"].clone()
}

async fn add_event(
    state: &AppState,
    token: &str,
    base: &str,
    shareholder: serde_json::Value,
    kind: &str,
    antall: i64,
) {
    let (status, body) = call(
        state,
        token,
        "POST",
        &format!("{base}/share-events"),
        Some(json!({
            "shareholder_id": shareholder, "type": kind, "dato": "2026-01-02",
            "antall": antall, "belop_ore": antall * 30_000
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// #182: the oppgave is stored before it is sent, a retry resends every
/// form with the SAME keys, a changed aksjeeierbok makes a NEW filing, and
/// a year with a receipt is not filed again.
#[tokio::test]
async fn the_return_is_filed_once_and_retried_with_the_same_keys() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Styreleder"), None)
        .await
        .unwrap();
    // The systembruker token needs a MOD11-valid number.
    let orgnumber = std::iter::repeat_with(unique_orgnumber)
        .find(|n| regnmed_core::orgnumber::is_valid(n))
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &orgnumber, "Innsending AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let token = idp.token(&sub, "Kari Styreleder");
    let base = format!("/companies/{company}");
    let submissions = format!("{base}/reports/aksjonaeroppgave/submissions?year=2026");

    let kari = add_shareholder(&state, &token, &base, "Kari Nordmann", "26829398612").await;
    let ola = add_shareholder(&state, &token, &base, "Ola Nordmann", "12855599579").await;
    add_event(&state, &token, &base, kari, "stiftelse", 100).await;

    // No rail: says so, stores nothing.
    let (status, body) = call(&state, &token, "POST", &submissions, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("ikke satt opp"),
        "{body}"
    );

    let (mock, calls) = mock_skatteetaten(2).await;
    state.shareholder_return = Some(std::sync::Arc::new(regnmed_api::ShareholderReturnRail {
        tokens: regnmed_gov::maskinporten::TokenProvider::new(
            regnmed_gov::maskinporten::MaskinportenConfig {
                token_endpoint: format!("{mock}/token"),
                audience: "https://test.maskinporten.no/".into(),
                client_id: "regnmed-test".into(),
                encoding_key: idp.encoding_key.clone(),
                kid: None,
                scopes: regnmed_gov::shareholder_return::SCOPES.into(),
            },
        ),
        client: regnmed_gov::shareholder_return::ShareholderReturnClient::new(
            format!("{mock}/v1"),
            format!("{mock}/dp"),
        ),
    }));

    // Bekreft fails twice. The second attempt resends the SAME keys.
    let (status, body) = call(&state, &token, "POST", &submissions, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("GLD_023"),
        "{body}"
    );
    let first: Vec<_> = calls.lock().unwrap().clone();
    assert_eq!(first.len(), 3, "H, one U, bekreft: {first:?}");
    let (status, _) = call(&state, &token, "POST", &submissions, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let second: Vec<_> = calls.lock().unwrap()[3..].to_vec();
    assert_eq!(
        first, second,
        "a retry is the same calls with the same keys"
    );

    let (_, body) = call(&state, &token, "GET", &submissions, None).await;
    let list = body["innsendinger"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{body}");
    assert_eq!(list[0]["status"], "ikke_levert");
    assert!(list[0]["siste_feil"].as_str().unwrap().contains("GLD_023"));

    // The aksjeeierbok changes: a NEW filing with new keys, now delivered.
    add_event(&state, &token, &base, ola, "nyemisjon", 50).await;
    let (status, body) = call(&state, &token, "POST", &submissions, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "levert");
    assert_eq!(body["antall_underskjema"], 2);
    assert_eq!(body["kvittering"]["forsendelse_id"], "f-1");
    let third: Vec<_> = calls.lock().unwrap()[6..].to_vec();
    assert_eq!(third.len(), 4, "H, two U, bekreft: {third:?}");
    assert!(
        third.iter().all(|c| first.iter().all(|f| f.1 != c.1)),
        "a changed oppgave never reuses a key"
    );
    let (_, body) = call(&state, &token, "GET", &submissions, None).await;
    assert_eq!(body["innsendinger"].as_array().unwrap().len(), 2);
    assert_eq!(body["innsendinger"][0]["status"], "levert");

    // Skatteetaten's answer comes from Dialogporten, and is stored.
    let delivered = body["innsendinger"][0]["id"].as_str().unwrap().to_string();
    let outcome = format!("{base}/reports/aksjonaeroppgave/submissions/{delivered}/outcome");
    let (status, body) = call(&state, &token, "POST", &outcome, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "godkjent");
    assert!(
        body["svar"]["tittel"]
            .as_str()
            .unwrap()
            .contains("godkjent")
    );
    assert_eq!(body["svar"]["dokumenter"][0][1], "https://x/t.pdf");

    // Delivered: not filed again.
    let (status, body) = call(&state, &token, "POST", &submissions, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("allerede levert"),
        "{body}"
    );
    assert_eq!(calls.lock().unwrap().len(), 10, "nothing more was sent");
}
