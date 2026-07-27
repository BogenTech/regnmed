//! Tilgangsmatrisen (#56): hvem slipper til hvor.
//!
//! Denne testen er skrevet som en **sperre mot regresjon i
//! autorisasjonen**, ikke som en funksjonstest. Da de 22 kopiene av
//! `require_access` ble slått sammen til én vakt (`regnmed_api::tilgang`)
//! fantes det ingen test som ville sagt fra om en av dem ble oversatt
//! feil — en `false` som ble til `Krav::Bokfor` ville stengt et
//! endepunkt, en `true` som ble til `Krav::Les` ville åpnet ett.
//!
//! Derfor er det NEKTELSENE som testes. At en admin slipper til er
//! dekket overalt ellers; at en leser IKKE slipper til er det ingenting
//! annet som fanger.
//!
//! Matrisen er også spesifikasjonen de neste sakene måles mot (#54
//! ansattrolle, #58 docs/auth.md).
//!
//! Krever DATABASE_URL; hopper over ellers.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn status(state: &AppState, method: &str, uri: &str, bearer: &str, body: &str) -> StatusCode {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    response.status()
}

/// A person with the given role on a fresh company.
async fn person_med_rolle(state: &AppState, idp: &TestIdp, company: Uuid, rolle: &str) -> String {
    let sub = format!("{rolle}|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some(rolle), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, rolle)
        .await
        .unwrap();
    idp.token(&sub, rolle)
}

/// Et **lesende** endepunkt per gruppe. Alle tre rollene skal slippe
/// inn; det er selve medlemskapet som gir lesetilgang.
const LESING: &[&str] = &[
    "/companies/{c}/invoices",
    "/companies/{c}/products",
    "/companies/{c}/assets",
    "/companies/{c}/dimensions",
    "/companies/{c}/parties",
    "/companies/{c}/period-lock",
    "/companies/{c}/employees",
    "/companies/{c}/payroll",
    "/companies/{c}/reports/saldobalanse?from=2026-01-01&to=2026-12-31",
    "/companies/{c}/settings",
];

/// Endepunkter som endrer noe. `les` skal få 403 — ikke 404, for
/// selskapet finnes og vedkommende har tilgang til det; det er nivået
/// som ikke rekker.
///
/// Kroppene må være **gyldige**: axum kjører `Json<T>`-uttrekket før
/// handleren, så en tom kropp gir 422 og vakten blir aldri spurt. En
/// test med `{}` ville altså bestått uten å bevise noe.
const SKRIVING: &[(&str, &str, &str)] = &[
    (
        "POST",
        "/companies/{c}/products",
        r#"{"nummer":"1","navn":"Vare","salgspris_ore":1000}"#,
    ),
    (
        "POST",
        "/companies/{c}/dimensions",
        r#"{"kind":"prosjekt","code":"P1","name":"Prosjekt"}"#,
    ),
    (
        "POST",
        "/companies/{c}/assets",
        r#"{"navn":"Maskin","anskaffelsesdato":"2026-01-01","kostpris_ore":100000,
            "levetid_maneder":60,"saldogruppe":"d"}"#,
    ),
    (
        "PUT",
        "/companies/{c}/period-lock",
        r#"{"locked_through":"2026-01-31"}"#,
    ),
    (
        "POST",
        "/companies/{c}/timesheet",
        r#"{"dato":"2026-01-15","minutter":60,"beskrivelse":"Arbeid"}"#,
    ),
];

/// Endepunkter bare en admin skal nå. Både `les` og `bokforing` skal
/// avvises — og det er `bokforing` som er den interessante: den har
/// full skrivetilgang til hovedboken og skal likevel ikke kunne slippe
/// inn en integrasjon eller endre firmaopplysningene.
const ADMIN: &[(&str, &str, &str)] = &[
    ("PUT", "/companies/{c}/settings", r#"{"address":"Gata 1"}"#),
    (
        "POST",
        "/companies/{c}/integrations",
        r#"{"client_id":"maskin","navn":"Robot","access":"les"}"#,
    ),
    (
        "POST",
        "/companies/{c}/attestering/policy",
        r#"{"aktiv":true,"belopsgrense_ore":100000}"#,
    ),
    (
        "POST",
        "/companies/{c}/mva/terminordning",
        r#"{"ordning":"arlig","valid_from":"2026-01-01"}"#,
    ),
    (
        "PUT",
        "/companies/{c}/timesheet/lock",
        r#"{"locked_through":"2026-01-31"}"#,
    ),
];

async fn oppsett() -> Option<(AppState, TestIdp, Uuid, String, String, String)> {
    let idp = TestIdp::new();
    let state = test_state(&idp).await?;
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Tilgangstest AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    let admin = person_med_rolle(&state, &idp, company, "admin").await;
    let bokforing = person_med_rolle(&state, &idp, company, "bokforing").await;
    let les = person_med_rolle(&state, &idp, company, "les").await;
    Some((state, idp, company, admin, bokforing, les))
}

#[tokio::test]
async fn alle_roller_far_lese() {
    let Some((state, _idp, company, admin, bokforing, les)) = oppsett().await else {
        return;
    };
    for uri in LESING {
        let uri = uri.replace("{c}", &company.to_string());
        for (navn, token) in [("admin", &admin), ("bokforing", &bokforing), ("les", &les)] {
            let s = status(&state, "GET", &uri, token, "").await;
            assert_ne!(s, StatusCode::FORBIDDEN, "{navn} nektet lesing av {uri}");
            assert_ne!(s, StatusCode::NOT_FOUND, "{navn} fikk 404 på {uri}");
        }
    }
}

/// Kjernen i saken: en revisor (som får `les` gjennom oppdraget sitt)
/// skal ikke kunne endre noe som helst.
#[tokio::test]
async fn lesetilgang_kan_ikke_endre_noe() {
    let Some((state, _idp, company, _admin, _bokforing, les)) = oppsett().await else {
        return;
    };
    for (method, uri, body) in SKRIVING {
        let uri = uri.replace("{c}", &company.to_string());
        assert_eq!(
            status(&state, method, &uri, &les, body).await,
            StatusCode::FORBIDDEN,
            "les skulle vært nektet {method} {uri}"
        );
    }
}

/// Bokføringstilgang er ikke administrasjon. Den som fører regnskapet
/// skal ikke kunne endre firmaopplysninger, slippe inn en integrasjon
/// eller sette attesteringspolicyen som skal kontrollere ham selv.
#[tokio::test]
async fn bokforing_er_ikke_administrasjon() {
    let Some((state, _idp, company, admin, bokforing, les)) = oppsett().await else {
        return;
    };
    for (method, uri, body) in ADMIN {
        let uri = uri.replace("{c}", &company.to_string());
        assert_eq!(
            status(&state, method, &uri, &bokforing, body).await,
            StatusCode::FORBIDDEN,
            "bokforing skulle vært nektet {method} {uri}"
        );
        assert_eq!(
            status(&state, method, &uri, &les, body).await,
            StatusCode::FORBIDDEN,
            "les skulle vært nektet {method} {uri}"
        );
        // Admin slipper forbi vakten. Hva som skjer etterpå er ikke
        // denne testens sak — poenget er at svaret IKKE er 403.
        assert_ne!(
            status(&state, method, &uri, &admin, body).await,
            StatusCode::FORBIDDEN,
            "admin skulle sluppet forbi vakten på {method} {uri}"
        );
    }
}

/// Uten tilgang skal selskapet ikke engang bekreftes å finnes — 404,
/// aldri 403. Ellers blir tilgangsfeilen en oppslagstjeneste over hvem
/// som er kunde hos oss.
#[tokio::test]
async fn utenforstaende_far_404_ikke_403() {
    let Some((state, idp, company, _admin, _bokforing, _les)) = oppsett().await else {
        return;
    };
    let sub = format!("fremmed|{}", Uuid::new_v4());
    regnmed_db::ensure_person(&state.pool, &sub, Some("Fremmed"), None)
        .await
        .unwrap();
    let fremmed = idp.token(&sub, "Fremmed");

    for uri in LESING {
        let uri = uri.replace("{c}", &company.to_string());
        assert_eq!(
            status(&state, "GET", &uri, &fremmed, "").await,
            StatusCode::NOT_FOUND,
            "{uri} lekket at selskapet finnes"
        );
    }
    for (method, uri, body) in SKRIVING {
        let uri = uri.replace("{c}", &company.to_string());
        assert_eq!(
            status(&state, method, &uri, &fremmed, body).await,
            StatusCode::NOT_FOUND,
            "{method} {uri} lekket at selskapet finnes"
        );
    }
}
