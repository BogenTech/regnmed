//! Aksjeeierbok and aksjonærregisteroppgave (#43): the holding is
//! computed and never stored, the events cannot be changed, the dividend
//! is posted in the same transaction as the decision, and the oppgave
//! that comes out validates against Skatteetaten's own XSDs.
//!
//! Requires DATABASE_URL — skips politely otherwise.

use crate::common::{TestIdp, test_state, unique_orgnr};
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
fn valider(xml: &str, xsd_navn: &str, tag: &str) {
    let xsd = format!(
        "{}/../../docs/aksjonaer/{xsd_navn}",
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
async fn aksjeeierbok_dividends_and_the_oppgave() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Styreleder"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Aksjeselskapet AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [
        ("2050", "Annen egenkapital"),
        ("2800", "Avsatt utbytte"),
        ("1920", "Bankinnskudd"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, nr, navn)
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

    // Utbytte: vedtaket bokføres i samme transaksjon.
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
    // The preview SHOWS what blocks the filing — it does not die of it.
    // The 2026 sale has no verified code.
    assert_eq!(body["leverbar"], false);
    let hindringer = body["hindringer"].as_array().unwrap();
    assert!(!hindringer.is_empty());
    assert!(
        hindringer
            .iter()
            .any(|h| h.as_str().unwrap().contains("salg")),
        "{hindringer:?}"
    );

    // The 2026 sale has no verified RF-1086 code — so we refuse, loudly,
    // rather than guess.
    let (status, body) = call(
        &state,
        &token,
        "GET",
        &format!("{base}/reports/aksjonaeroppgave?year=2026&format=xml"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let feil = body["error"].as_str().unwrap();
    assert!(feil.contains("salg"), "{feil}");
    assert!(feil.contains("gjetter"), "{feil}");

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

    let under = body["underskjemaer"].as_array().unwrap();
    assert_eq!(under.len(), 2);
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
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Uforanderlig AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();

    let holder = regnmed_db::aksjebok::create_aksjonaer(
        &state.pool,
        company,
        &regnmed_db::aksjebok::NyAksjonaer {
            kind: "selskap".into(),
            navn: "Eier AS".into(),
            fodselsnummer: None,
            orgnr: Some("923609016".into()),
            utenlandsk_id: None,
            adresse: None,
            postnummer: None,
            poststed: None,
            landkode: None,
            note: None,
        },
        "Ola",
    )
    .await
    .unwrap();
    let event = regnmed_db::aksjebok::record_hendelse(
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
    regnmed_db::aksjebok::update_aksjonaer_kontakt(
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
