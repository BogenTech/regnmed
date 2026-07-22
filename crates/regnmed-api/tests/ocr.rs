//! End-to-end OCR giro over the web API: upload a payment file, list the
//! KID-tagged payments, duplicate upload rejected, permissions enforced.
//! Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

fn record(parts: &[&str]) -> String {
    let line: String = parts.concat();
    assert_eq!(line.len(), 80, "fixture record must be 80 chars: {line}");
    line
}

/// One oppdrag with a single 12 500,00 kr payment on a mod10 KID. The
/// transmission number is randomized so parallel test runs don't collide
/// on the idempotency constraint.
fn ocr_file(transmission: &str) -> String {
    [
        record(&[
            "NY000010",
            "00111222",
            transmission,
            "00008080",
            &"0".repeat(49),
        ]),
        record(&[
            "NY090020",
            "000988555",
            "0000001",
            "99991042764",
            &"0".repeat(45),
        ]),
        record(&[
            "NY091030",
            "0000001",
            "200126",
            "00",
            "20",
            "0",
            "00001",
            "0",
            "00000000001250000",
            "                  1234566",
            "000000",
        ]),
        record(&[
            "NY090088",
            "00000001",
            "00000003",
            "00000000001250000",
            "200126",
            "200126",
            "200126",
            &"0".repeat(21),
        ]),
        record(&[
            "NY000089",
            "00000001",
            "00000005",
            "00000000001250000",
            "200126",
            &"0".repeat(33),
        ]),
    ]
    .join("\n")
}

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
) -> (StatusCode, serde_json::Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::from(body.unwrap_or_default()))
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

#[tokio::test]
async fn upload_list_duplicate_and_permissions() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "OCR Klient AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    regnmed_db::ensure_account(&state.pool, company, "1920", "Bankinnskudd")
        .await
        .unwrap();

    let token = idp.token(&sub, "Kari Bokfører");
    let transmission = format!("{:07}", rand_7());
    let file = ocr_file(&transmission);

    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/ocr/files?account=1920"),
        &token,
        Some(file.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["batches"], 1);
    assert_eq!(body["payments"], 1);
    assert_eq!(body["sum_ore"], 1_250_000);
    assert_eq!(body["kid_invalid"], 0);

    // Duplicate upload of the same oppdrag: rejected.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/ocr/files?account=1920"),
        &token,
        Some(file),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Listing shows the KID payment.
    let (status, list) = request(
        &state,
        "GET",
        &format!("/companies/{company}/ocr/payments?from=2026-01-01&to=2026-12-31"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let payment = list["payments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["kid"] == "1234566")
        .expect("uploaded payment listed");
    assert_eq!(payment["amount_ore"], 1_250_000);
    assert_eq!(payment["kid_valid"], true);
    assert_eq!(payment["account"], "1920");

    // A stranger sees nothing.
    let stranger = idp.token(&format!("test|{}", Uuid::new_v4()), "Ukjent Person");
    let (status, _) = request(
        &state,
        "GET",
        &format!("/companies/{company}/ocr/payments"),
        &stranger,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

fn rand_7() -> u32 {
    u32::from_be_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap()) % 10_000_000
}
