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

// ---------------------------------------------------------------------
// Ansattrollen (#54).
//
// Det som er verdt å teste er NEKTELSENE. At en ansatt får føre sine
// egne timer er lett å se i portalen; at hun ikke kommer til hovedboken,
// kollegenes timer eller andres lønnsslipp er det ingenting annet som
// fanger.
// ---------------------------------------------------------------------

/// Alt en ansatt IKKE skal nå. Listen er med vilje bred: den dekker
/// hovedbok, rapporter, penger, register og alles data.
/// Merk at spørrestrengene må være **gyldige**, av samme grunn som
/// kroppene i SKRIVING: axum kjører `Query<T>`-uttrekket før handleren,
/// så en manglende parameter gir 400 og vakten blir aldri spurt. Da
/// ville testen bestått uten å ha bevist noe.
const NEKTET_FOR_ANSATT: &[(&str, &str)] = &[
    ("GET", "/companies/{c}/vouchers"),
    (
        "GET",
        "/companies/{c}/reports/saldobalanse?from=2026-01-01&to=2026-12-31",
    ),
    (
        "GET",
        "/companies/{c}/reports/resultat?from=2026-01-01&to=2026-12-31",
    ),
    ("GET", "/companies/{c}/invoices"),
    ("GET", "/companies/{c}/parties"),
    ("GET", "/companies/{c}/bank/reconciliation?account=1920"),
    ("GET", "/companies/{c}/payments/runs"),
    ("GET", "/companies/{c}/products"),
    ("GET", "/companies/{c}/assets"),
    ("GET", "/companies/{c}/employees"),
    ("GET", "/companies/{c}/payroll"),
    ("GET", "/companies/{c}/settings"),
    ("GET", "/companies/{c}/inbox"),
    ("GET", "/companies/{c}/access"),
    // Selskapsvide timeoversikter: totalene per prosjekt og det
    // ufakturerte er ikke den ansattes sak.
    (
        "GET",
        "/companies/{c}/timesheet/summary?from=2026-01-01&to=2026-12-31",
    ),
    ("GET", "/companies/{c}/timesheet/unbilled"),
];

/// Og det hun SKAL nå — selvbetjeningen, som ellers ville vært verdiløs.
const TILLATT_FOR_ANSATT: &[&str] = &[
    "/companies/{c}/timesheet?from=2026-01-01&to=2026-12-31",
    "/companies/{c}/timesheet/lock",
    "/companies/{c}/expenses",
    "/companies/{c}/dimensions",
];

#[tokio::test]
async fn ansatt_kommer_ikke_til_hovedboken() {
    let Some((state, idp, company, _admin, _bokforing, _les)) = oppsett().await else {
        return;
    };
    let ansatt = person_med_rolle(&state, &idp, company, "ansatt").await;

    for (method, uri) in NEKTET_FOR_ANSATT {
        let uri = uri.replace("{c}", &company.to_string());
        assert_eq!(
            status(&state, method, &uri, &ansatt, "").await,
            StatusCode::FORBIDDEN,
            "ansatt skulle vært nektet {method} {uri}"
        );
    }
}

#[tokio::test]
async fn ansatt_far_gjore_sitt_eget() {
    let Some((state, idp, company, _admin, _bokforing, _les)) = oppsett().await else {
        return;
    };
    let ansatt = person_med_rolle(&state, &idp, company, "ansatt").await;

    for uri in TILLATT_FOR_ANSATT {
        let uri = uri.replace("{c}", &company.to_string());
        let s = status(&state, "GET", &uri, &ansatt, "").await;
        assert_ne!(s, StatusCode::FORBIDDEN, "ansatt nektet {uri}");
        assert_ne!(s, StatusCode::NOT_FOUND, "ansatt fikk 404 på {uri}");
    }

    // Føre en egen time, og sende inn et eget utlegg.
    assert_ne!(
        status(
            &state,
            "POST",
            &format!("/companies/{company}/timesheet"),
            &ansatt,
            r#"{"dato":"2026-01-15","minutter":60,"beskrivelse":"Arbeid"}"#,
        )
        .await,
        StatusCode::FORBIDDEN,
        "ansatt skulle fått føre sine egne timer"
    );
    assert_ne!(
        status(
            &state,
            "POST",
            &format!("/companies/{company}/expenses/kjoring"),
            &ansatt,
            r#"{"dato":"2026-01-15","beskrivelse":"Kundebesøk","km":10}"#,
        )
        .await,
        StatusCode::FORBIDDEN,
        "ansatt skulle fått sende inn eget utlegg"
    );
}

/// Å laste opp en kvittering er ikke å bokføre den. Dokumentet havner i
/// innboksen og venter på noen med BILAG_BOKFOR — den ansatte kommer
/// verken til innboksen eller til bokføringen.
#[tokio::test]
async fn ansatt_kan_laste_opp_men_ikke_bokfore() {
    let Some((state, idp, company, _admin, _bokforing, _les)) = oppsett().await else {
        return;
    };
    let ansatt = person_med_rolle(&state, &idp, company, "ansatt").await;

    let opplasting = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/companies/{company}/inbox?filename=kvittering.pdf"
                ))
                .header("authorization", format!("Bearer {ansatt}"))
                .header("content-type", "application/pdf")
                .body(Body::from("kvittering"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        opplasting.status(),
        StatusCode::OK,
        "opplasting skulle gått"
    );

    // Men innboksen er ikke hennes å lese.
    assert_eq!(
        status(
            &state,
            "GET",
            &format!("/companies/{company}/inbox"),
            &ansatt,
            ""
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

/// Omfanget, ikke bare rettigheten: en ansatt med LONNSSLIPP_LES_EGEN
/// skal få SIN slipp og ikke kollegaens. Dette er selve grunnen til at
/// `_EGNE`/`_ALLE` finnes — uten det ville rettigheten enten gitt alt
/// eller ingenting.
#[tokio::test]
async fn ansatt_far_bare_sin_egen_lonnsslipp() {
    let Some((state, idp, company, admin, _bokforing, _les)) = oppsett().await else {
        return;
    };
    for (nr, navn) in [
        ("5000", "Lønn"),
        ("5090", "Feriepenger"),
        ("5400", "Aga"),
        ("2600", "Trekk"),
        ("2770", "Skyldig aga"),
        ("2930", "Skyldig lønn"),
        ("2940", "Skyldige feriepenger"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, nr, navn)
            .await
            .unwrap();
    }

    // To ansatte, begge koblet til hver sin portalbruker.
    let mut employees = Vec::new();
    for (fnr, navn) in [("03048810003", "Ansatt En"), ("03048810194", "Ansatt To")] {
        let sub = format!("{navn}|{}", Uuid::new_v4());
        let person = regnmed_db::ensure_person(&state.pool, &sub, Some(navn), None)
            .await
            .unwrap();
        regnmed_db::ensure_company_member(&state.pool, company, person, "ansatt")
            .await
            .unwrap();
        let employee = regnmed_db::lonn::create_ansatt(
            &state.pool,
            company,
            &regnmed_db::lonn::NyAnsatt {
                fodselsnummer: fnr.into(),
                navn: navn.into(),
                stilling: None,
                ansatt_fra: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                manedslonn_ore: Some(5_000_000),
                timelonn_ore: None,
                trekk_type: "prosent".into(),
                trekk_prosent_bp: Some(3000),
                trekk_tabell: None,
                feriepenger_bp: 1020,
                bank_account: None,
                note: None,
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
        employees.push((employee, idp.token(&sub, navn)));
    }

    let kjoring = regnmed_db::lonn::kjor_lonn(
        &state.pool,
        company,
        2026,
        3,
        chrono::NaiveDate::from_ymd_opt(2026, 3, 25).unwrap(),
        "I",
        &employees
            .iter()
            .map(|(id, _)| regnmed_db::lonn::Lonnspost {
                employee_id: *id,
                brutto_ore: None,
                feriepenger_ore: 0,
                fra_timer: false,
            })
            .collect::<Vec<_>>(),
        "Test",
    )
    .await
    .unwrap();

    let (en_id, en_token) = &employees[0];
    let (to_id, _) = &employees[1];
    let run = kjoring.id;

    // Sin egen: ja.
    assert_eq!(
        status(
            &state,
            "GET",
            &format!("/companies/{company}/payroll/{run}/slip/{en_id}"),
            en_token,
            "",
        )
        .await,
        StatusCode::OK,
        "ansatt skulle fått sin egen lønnsslipp"
    );
    // Kollegaens: 404, ikke 403 — hun skal ikke få vite at den finnes.
    assert_eq!(
        status(
            &state,
            "GET",
            &format!("/companies/{company}/payroll/{run}/slip/{to_id}"),
            en_token,
            "",
        )
        .await,
        StatusCode::NOT_FOUND,
        "ansatt skulle ikke nådd kollegaens lønnsslipp"
    );
    // Admin har LONNSSLIPP_LES_ALLE og når begge — dagens oppførsel,
    // uendret av #54 (det er #55 som skal snevre den inn).
    assert_eq!(
        status(
            &state,
            "GET",
            &format!("/companies/{company}/payroll/{run}/slip/{to_id}"),
            &admin,
            "",
        )
        .await,
        StatusCode::OK
    );
}
