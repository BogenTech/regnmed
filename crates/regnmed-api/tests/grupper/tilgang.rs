//! The access matrix (#56): who gets in where.
//!
//! This test is written as a **regression guard on authorization**, not
//! as a functional test. When the 22 copies of `require_access` were
//! merged into one guard (`regnmed_api::tilgang`) there was no test that
//! would have said anything if one of them had been translated wrong — a
//! `false` that became `Krav::Bokfor` would have closed an endpoint, a
//! `true` that became `Krav::Les` would have opened one.
//!
//! So it is the REFUSALS that are tested. That an admin gets in is
//! covered everywhere else; that a reader does NOT is caught by nothing
//! else.
//!
//! The matrix is also the specification the following issues are measured
//! against (#54 the ansatt role, #58 docs/auth.md).
//!
//! Requires DATABASE_URL; skips otherwise.

use crate::common::{TestIdp, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

/// Like [`status`], but with the response body — for endpoints where the
/// error message is half the point.
async fn json_call(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: &str,
) -> (StatusCode, serde_json::Value) {
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
    let kode = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        kode,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

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
async fn person_with_role(state: &AppState, idp: &TestIdp, company: Uuid, rolle: &str) -> String {
    let sub = format!("{rolle}|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some(rolle), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, rolle)
        .await
        .unwrap();
    idp.token(&sub, rolle)
}

/// One **reading** endpoint per group. All three roles must get in; it is
/// membership itself that grants read access.
const LESING: &[&str] = &[
    "/companies/{c}/invoices",
    "/companies/{c}/products",
    "/companies/{c}/assets",
    "/companies/{c}/dimensions",
    "/companies/{c}/parties",
    "/companies/{c}/period-lock",
    "/companies/{c}/reports/saldobalanse?from=2026-01-01&to=2026-12-31",
    "/companies/{c}/settings",
];

/// Endpoints that change something. `les` must get 403 — not 404, since
/// the company exists and the person has access to it; it is the level
/// that falls short.
///
/// The bodies must be **valid**: axum runs the `Json<T>` extractor before
/// the handler, so an empty body gives 422 and the guard is never asked.
/// A test with `{}` would therefore have passed without proving
/// anything.
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

/// Endpoints only an admin should reach. Both `les` and `bokforing` must
/// be refused — and `bokforing` is the interesting one: it has full write
/// access to the hovedbok and must still not be able to let in an
/// integration or change the company details.
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

async fn setup() -> Option<(AppState, TestIdp, Uuid, String, String, String)> {
    let idp = TestIdp::new();
    let state = test_state(&idp).await?;
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Tilgangstest AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    let admin = person_with_role(&state, &idp, company, "admin").await;
    let bokforing = person_with_role(&state, &idp, company, "bokforing").await;
    let les = person_with_role(&state, &idp, company, "les").await;
    Some((state, idp, company, admin, bokforing, les))
}

#[tokio::test]
async fn every_role_may_read() {
    let Some((state, _idp, company, admin, bokforing, les)) = setup().await else {
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

/// The heart of the matter: a revisor (who gets `les` through their
/// oppdrag) must not be able to change anything at all.
#[tokio::test]
async fn read_access_cannot_change_anything() {
    let Some((state, _idp, company, _admin, _bokforing, les)) = setup().await else {
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

/// Posting access is not administration. Whoever keeps the accounts must
/// not be able to change company details, let in an integration, or set
/// the attestering policy that is meant to check them.
#[tokio::test]
async fn bokforing_is_not_administration() {
    let Some((state, _idp, company, admin, bokforing, les)) = setup().await else {
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
        // Admin gets past the guard. What happens afterwards is not this
        // test's business — the point is that the answer is NOT 403.
        assert_ne!(
            status(&state, method, &uri, &admin, body).await,
            StatusCode::FORBIDDEN,
            "admin skulle sluppet forbi vakten på {method} {uri}"
        );
    }
}

/// Without access the company must not even be confirmed to exist — 404,
/// never 403. Otherwise the access error becomes a lookup service for who
/// is a customer of ours.
#[tokio::test]
async fn an_outsider_gets_404_not_403() {
    let Some((state, idp, company, _admin, _bokforing, _les)) = setup().await else {
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
// The ansatt role (#54).
//
// What is worth testing are the REFUSALS. That an employee may log their
// own hours is easy to see in the portal; that they cannot reach the
// hovedbok, their colleagues' hours or other people's payslips is caught
// by nothing else.
// ---------------------------------------------------------------------

/// Everything an employee must NOT reach. The list is deliberately
/// broad: it covers the hovedbok, reports, money, registers and everyone
/// else's data. Note that the query strings must be **valid**, for the
/// same reason as the bodies in SKRIVING: axum runs the `Query<T>`
/// extractor before the handler, so a missing parameter gives 400 and the
/// guard is never asked. The test would then have passed without proving
/// anything.
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
    // Company-wide hour overviews: the totals per prosjekt and what is
    // unbilled are not the employee's business.
    (
        "GET",
        "/companies/{c}/timesheet/summary?from=2026-01-01&to=2026-12-31",
    ),
    ("GET", "/companies/{c}/timesheet/unbilled"),
];

/// And what they SHALL reach — the self-service, which would be useless otherwise.
const TILLATT_FOR_ANSATT: &[&str] = &[
    "/companies/{c}/timesheet?from=2026-01-01&to=2026-12-31",
    "/companies/{c}/timesheet/lock",
    "/companies/{c}/expenses",
    "/companies/{c}/dimensions",
];

#[tokio::test]
async fn an_ansatt_cannot_reach_the_hovedbok() {
    let Some((state, idp, company, _admin, _bokforing, _les)) = setup().await else {
        return;
    };
    let ansatt = person_with_role(&state, &idp, company, "ansatt").await;

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
async fn an_ansatt_may_do_their_own() {
    let Some((state, idp, company, _admin, _bokforing, _les)) = setup().await else {
        return;
    };
    let ansatt = person_with_role(&state, &idp, company, "ansatt").await;

    for uri in TILLATT_FOR_ANSATT {
        let uri = uri.replace("{c}", &company.to_string());
        let s = status(&state, "GET", &uri, &ansatt, "").await;
        assert_ne!(s, StatusCode::FORBIDDEN, "ansatt nektet {uri}");
        assert_ne!(s, StatusCode::NOT_FOUND, "ansatt fikk 404 på {uri}");
    }

    // Log an hour of their own, and submit an utlegg of their own.
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

/// Uploading a receipt is not posting it. The document lands in the
/// innboks and waits for somebody with BILAG_BOKFOR — the employee
/// reaches neither the innboks nor the posting.
#[tokio::test]
async fn an_ansatt_may_upload_but_not_post() {
    let Some((state, idp, company, _admin, _bokforing, _les)) = setup().await else {
        return;
    };
    let ansatt = person_with_role(&state, &idp, company, "ansatt").await;

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

    // But the innboks is not hers to read.
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

/// The scope, not just the rettighet: an employee with
/// LONNSSLIPP_LES_EGEN must get THEIR slip and not a colleague's. This is
/// the very reason `_EGNE`/`_ALLE` exists — without it the rettighet
/// would grant either everything or nothing.
#[tokio::test]
async fn an_ansatt_gets_only_their_own_payslip() {
    let Some((state, idp, company, admin, _bokforing, _les)) = setup().await else {
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

    // Two employees, each linked to their own portal user.
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

    // Their own: yes.
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
    // The colleague's: 404, not 403 — she must not learn it exists.
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
    // Admin has LONNSSLIPP_LES_ALLE and reaches both — today's behavior,
    // unchanged by #54 (it is #55 that narrows it).
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

/// Lønn is not general reading (#55).
///
/// Before this, anyone with read access could download anybody's payslip
/// and see the employee list with birth date, monthly salary and
/// withholding percentage. Now we separate them: a **revisor** sees lønn,
/// because it is subject to audit, while an **internal reader** does
/// not.
#[tokio::test]
async fn lonn_is_not_general_reading() {
    let Some((state, idp, company, admin, bokforing, les)) = setup().await else {
        return;
    };

    // A revisor comes in through an oppdrag, not as a member.
    let firm = regnmed_db::ensure_firm(&state.pool, &unique_orgnr(), "Revisjon AS", "revisjon")
        .await
        .unwrap();
    let sub = format!("revisor|{}", Uuid::new_v4());
    let rp = regnmed_db::ensure_person(&state.pool, &sub, Some("Revisor"), None)
        .await
        .unwrap();
    regnmed_db::ensure_firm_member(&state.pool, firm, rp, "ansatt")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(&state.pool, firm, company, "revisjon")
        .await
        .unwrap();
    let revisor = idp.token(&sub, "Revisor");
    let ansatt = person_with_role(&state, &idp, company, "ansatt").await;

    for uri in ["/companies/{c}/employees", "/companies/{c}/payroll"] {
        let uri = uri.replace("{c}", &company.to_string());
        // Refused: internal reader and employee.
        for (navn, token) in [("les", &les), ("ansatt", &ansatt)] {
            assert_eq!(
                status(&state, "GET", &uri, token, "").await,
                StatusCode::FORBIDDEN,
                "{navn} skulle vært nektet {uri}"
            );
        }
        // Allowed: revisor (audit duty), bokføring and admin.
        for (navn, token) in [
            ("revisor", &revisor),
            ("bokforing", &bokforing),
            ("admin", &admin),
        ] {
            assert_ne!(
                status(&state, "GET", &uri, token, "").await,
                StatusCode::FORBIDDEN,
                "{navn} skulle nådd {uri}"
            );
        }
    }

    // The revisor is still read-only — access to lønn is reading, not an
    // upgrade.
    assert_eq!(
        status(
            &state,
            "POST",
            &format!("/companies/{company}/employees"),
            &revisor,
            r#"{"fodselsnummer":"03048810003","navn":"X","ansatt_fra":"2026-01-01"}"#,
        )
        .await,
        StatusCode::FORBIDDEN,
        "revisor skulle ikke fått registrere ansatte"
    );
}

// ---------------------------------------------------------------------
// Custom roles (#60).
// ---------------------------------------------------------------------

/// "Someone who only invoices" — a role we never thought of, composed by
/// the company itself, and it works.
#[tokio::test]
async fn a_custom_role_grants_exactly_what_it_says() {
    let Some((state, idp, company, admin, _bokforing, _les)) = setup().await else {
        return;
    };

    let (kode, svar) = json_call(
        &state,
        "POST",
        &format!("/companies/{company}/roles"),
        &admin,
        r#"{"navn":"Fakturaansvarlig",
            "rettigheter":["FAKTURA_LES","FAKTURA_SKRIV","RESKONTRO_LES"]}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");

    // Give the role to somebody.
    let sub = format!("faktura|{}", Uuid::new_v4());
    let p = regnmed_db::ensure_person(&state.pool, &sub, Some("Fakturafolk"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, p, "Fakturaansvarlig")
        .await
        .unwrap();
    let token = idp.token(&sub, "Fakturafolk");

    // What the role grants.
    for uri in ["/companies/{c}/invoices", "/companies/{c}/parties"] {
        let uri = uri.replace("{c}", &company.to_string());
        assert_ne!(
            status(&state, "GET", &uri, &token, "").await,
            StatusCode::FORBIDDEN,
            "rollen skulle gitt {uri}"
        );
    }
    // And everything it does not — including neighbouring features it is
    // easy to assume come along.
    for uri in [
        "/companies/{c}/vouchers",
        "/companies/{c}/reports/saldobalanse?from=2026-01-01&to=2026-12-31",
        "/companies/{c}/products",
        "/companies/{c}/payroll",
        "/companies/{c}/bank/reconciliation?account=1920",
        "/companies/{c}/access",
    ] {
        let uri = uri.replace("{c}", &company.to_string());
        assert_eq!(
            status(&state, "GET", &uri, &token, "").await,
            StatusCode::FORBIDDEN,
            "rollen skulle IKKE gitt {uri}"
        );
    }
}

/// A role that can change access can give itself everything else. So
/// those rettigheter cannot go into a custom role at all — refused when
/// the role is created, not merely ignored on lookup.
#[tokio::test]
async fn access_governing_rettigheter_cannot_be_delegated() {
    let Some((state, _idp, company, admin, _bokforing, _les)) = setup().await else {
        return;
    };
    for farlig in [
        "MEDLEM_ADMIN",
        "SELSKAP_ADMIN",
        "OPPDRAG_ADMIN",
        "INTEGRASJON_ADMIN",
    ] {
        let (kode, svar) = json_call(
            &state,
            "POST",
            &format!("/companies/{company}/roles"),
            &admin,
            &format!(r#"{{"navn":"Farlig {farlig}","rettigheter":["{farlig}"]}}"#),
        )
        .await;
        assert_eq!(kode, StatusCode::BAD_REQUEST, "{farlig}: {svar}");
        assert!(
            svar["error"]
                .as_str()
                .unwrap_or_default()
                .contains("hvem som har tilgang"),
            "{svar}"
        );
    }

    // And a name that is not in the vocabulary is refused loudly here —
    // a human is writing, and a role that silently lacks half its
    // contents is worse than an error message.
    let (kode, svar) = json_call(
        &state,
        "POST",
        &format!("/companies/{company}/roles"),
        &admin,
        r#"{"navn":"Tullerolle","rettigheter":["FAKTURA_ALT"]}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::BAD_REQUEST, "{svar}");
    assert!(
        svar["error"]
            .as_str()
            .unwrap_or_default()
            .contains("ukjent rettighet"),
        "{svar}"
    );
}

/// Every built-in role explains itself (#79): the invitation guidance
/// renders these texts, and an empty one would leave the admin guessing
/// what they are granting.
#[tokio::test]
async fn every_builtin_role_describes_itself() {
    let Some((state, _idp, company, admin, _bokforing, _les)) = setup().await else {
        return;
    };
    let (kode, svar) = json_call(
        &state,
        "GET",
        &format!("/companies/{company}/roles"),
        &admin,
        "",
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    let innebygde = svar["innebygde"].as_array().unwrap();
    assert!(!innebygde.is_empty());
    for rolle in innebygde {
        let beskrivelse = rolle["beskrivelse"].as_str().unwrap_or_default();
        assert!(
            !beskrivelse.is_empty(),
            "rollen {} mangler beskrivelse",
            rolle["navn"]
        );
    }
}

/// A deactivated role grants nothing — that is how a role is "removed"
/// without losing the history of who held it.
#[tokio::test]
async fn a_deactivated_role_grants_no_access() {
    let Some((state, idp, company, admin, _bokforing, _les)) = setup().await else {
        return;
    };
    let (_, svar) = json_call(
        &state,
        "POST",
        &format!("/companies/{company}/roles"),
        &admin,
        r#"{"navn":"Midlertidig","rettigheter":["FAKTURA_LES"]}"#,
    )
    .await;
    let role_id = svar["role_id"].as_str().unwrap().to_string();

    let sub = format!("midl|{}", Uuid::new_v4());
    let p = regnmed_db::ensure_person(&state.pool, &sub, Some("Midl"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, p, "Midlertidig")
        .await
        .unwrap();
    let token = idp.token(&sub, "Midl");
    let uri = format!("/companies/{company}/invoices");
    assert_ne!(
        status(&state, "GET", &uri, &token, "").await,
        StatusCode::FORBIDDEN
    );

    let (kode, _) = json_call(
        &state,
        "POST",
        &format!("/companies/{company}/roles/{role_id}/deactivate"),
        &admin,
        "{}",
    )
    .await;
    assert_eq!(kode, StatusCode::OK);

    // The membership stands, but the role grants nothing now. Note 404,
    // not 403: without a single rettighet the person is no longer someone
    // who "has access, but not enough".
    assert_ne!(
        status(&state, "GET", &uri, &token, "").await,
        StatusCode::OK,
        "en deaktivert rolle skal ikke gi tilgang"
    );
}

// ---------------------------------------------------------------------
// No platform administrator (#57).
// ---------------------------------------------------------------------

/// No access route crosses a company boundary. The strongest role there
/// is — admin — is a COMPLETE stranger in the neighbouring company: 404
/// on everything, exactly like someone with no access anywhere.
///
/// The test is the decision's guard, not merely its illustration: a
/// future platform role, a wildcard route in the access lookup or a
/// forgotten company_id filter would all show up here. Should that
/// decision ever be reversed, this test must be changed deliberately —
/// and docs/auth.md §8 along with it.
#[tokio::test]
async fn an_admin_crosses_no_company_boundary() {
    let Some((state, _idp, company, admin, _bokforing, _les)) = setup().await else {
        return;
    };
    let nabo = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Naboselskapet AS")
        .await
        .unwrap();

    for uri in LESING {
        let uri = uri.replace("{c}", &nabo.to_string());
        assert_eq!(
            status(&state, "GET", &uri, &admin, "").await,
            StatusCode::NOT_FOUND,
            "admin i et annet selskap skulle fått 404 på {uri}"
        );
    }
    for (method, uri, body) in SKRIVING {
        let uri = uri.replace("{c}", &nabo.to_string());
        assert_eq!(
            status(&state, method, &uri, &admin, body).await,
            StatusCode::NOT_FOUND,
            "admin i et annet selskap skulle fått 404 på {method} {uri}"
        );
    }
    for (method, uri, body) in ADMIN {
        let uri = uri.replace("{c}", &nabo.to_string());
        assert_eq!(
            status(&state, method, &uri, &admin, body).await,
            StatusCode::NOT_FOUND,
            "admin i et annet selskap skulle fått 404 på {method} {uri}"
        );
    }

    // And /me does not mention the neighbouring company — the access list
    // is the companies the person can actually act for, no more.
    let (kode, me) = json_call(&state, "GET", "/me", &admin, "").await;
    assert_eq!(kode, StatusCode::OK);
    let ids: Vec<&str> = me["companies"]
        .as_array()
        .expect("companies")
        .iter()
        .filter_map(|c| c["company_id"].as_str())
        .collect();
    assert!(ids.contains(&company.to_string().as_str()));
    assert!(
        !ids.contains(&nabo.to_string().as_str()),
        "/me skulle ikke nevnt naboselskapet"
    );
}

/// The emergency procedure (#57, migration 0040) leaves a trail that is
/// named for what it is — and an emergency entry without a consent
/// reference is refused by the database itself. Without that, the access
/// log could lie about the very entry it exists to catch.
#[tokio::test]
async fn the_emergency_procedure_requires_a_reference_and_names_itself() {
    let Some((state, idp, company, admin, _bokforing, _les)) = setup().await else {
        return;
    };
    let sub = format!("gjenoppretting|{}", Uuid::new_v4());
    let p = regnmed_db::ensure_person(&state.pool, &sub, Some("Ny Admin"), None)
        .await
        .unwrap();

    // Without a reference: refused by the check constraint.
    let uten = sqlx::query(
        "insert into company_member_change
             (id, company_id, person_id, endring, til_rolle, kilde)
         values ($1,$2,$3,'lagt_til','admin','nodprosedyre')",
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(p)
    .execute(&state.pool)
    .await;
    assert!(
        uten.is_err(),
        "et nødinnslag uten samtykkereferanse skulle vært avvist"
    );

    // The procedure as documented in docs/auth.md §8.
    let mut tx = state.pool.begin().await.unwrap();
    sqlx::query(
        "insert into company_member (company_id, person_id, role)
         values ($1,$2,'admin')
         on conflict (company_id, person_id)
             do update set role = 'admin', active = true",
    )
    .bind(company)
    .bind(p)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "insert into company_member_change
             (id, company_id, person_id, endring, til_rolle, kilde, notat)
         values ($1,$2,$3,'lagt_til','admin','nodprosedyre',
                 'Samtykke fra styreleder 2026-07-28, ref SAK-123')",
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(p)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    // Access works, and the trail stands with its kilde and reference —
    // visible through the same endpoint as every other access change.
    let token = idp.token(&sub, "Ny Admin");
    assert_eq!(
        status(
            &state,
            "GET",
            &format!("/companies/{company}/access"),
            &token,
            ""
        )
        .await,
        StatusCode::OK
    );
    let (_, hist) = json_call(
        &state,
        "GET",
        &format!("/companies/{company}/access/history"),
        &admin,
        "",
    )
    .await;
    let innslag = hist["endringer"]
        .as_array()
        .expect("endringer")
        .iter()
        .find(|e| e["kilde"] == "nodprosedyre")
        .expect("nødinnslaget skulle stått i tilgangsloggen");
    assert!(
        innslag["notat"]
            .as_str()
            .unwrap_or_default()
            .contains("SAK-123"),
        "referansen skulle fulgt innslaget: {innslag}"
    );
}

// ---------------------------------------------------------------------
// One transaction per role change (#62).
// ---------------------------------------------------------------------

/// A role that exists without the change log explaining how is exactly
/// what the log exists to make impossible. If writing the log fails, the
/// role must not come into being.
///
/// The failure is provoked the way it would arise in production:
/// `utfort_av` points at a person who does not exist, and the foreign key
/// rejects the log row — i.e. the third step, after both the role and its
/// rettigheter have been written.
#[tokio::test]
async fn a_role_without_a_log_row_never_comes_into_being() {
    let Some((state, _idp, company, _admin, _bokforing, _les)) = setup().await else {
        return;
    };
    let spokelse = Uuid::new_v4();
    let res = regnmed_db::roller::opprett(
        &state.pool,
        company,
        "Halvferdig",
        &["FAKTURA_LES".to_string()],
        spokelse,
        "Testadmin",
    )
    .await;
    assert!(res.is_err(), "loggraden skulle ikke gått gjennom");

    let roller = regnmed_db::roller::list_roller(&state.pool, company)
        .await
        .unwrap();
    assert!(
        !roller.iter().any(|r| r.navn == "Halvferdig"),
        "rollen står igjen uten spor i loggen: {:?}",
        roller.iter().map(|r| &r.navn).collect::<Vec<_>>()
    );
}

/// The access guard must never see the rettighet list mid-rewrite.
///
/// `sett_rettigheter` is `delete` + `insert`; outside a transaction a
/// concurrent lookup can read in between and see an EMPTY list — whoever
/// holds the role loses access for a moment, at random, in an entirely
/// different request.
///
/// The test holds the role locked the way a concurrent change would, and
/// reads while the write stands waiting: the answer must be the old list,
/// the whole time. Without the lock and the transaction the write gets
/// past immediately, and the read sees either nothing or the new list —
/// both fail here.
#[tokio::test]
async fn rettigheter_are_never_read_half_written() {
    let Some((state, _idp, company, admin, _bokforing, _les)) = setup().await else {
        return;
    };
    let (kode, svar) = json_call(
        &state,
        "POST",
        &format!("/companies/{company}/roles"),
        &admin,
        r#"{"navn":"Kasserer","rettigheter":["FAKTURA_LES","FAKTURA_SKRIV"]}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    let role_id: Uuid = svar["role_id"].as_str().unwrap().parse().unwrap();
    let navn = vec!["Kasserer".to_string()];

    let sortert = |mut r: Vec<String>| {
        r.sort();
        r
    };
    let som_skriver = regnmed_db::ensure_person(
        &state.pool,
        &format!("skriver|{}", Uuid::new_v4()),
        Some("Skriver"),
        None,
    )
    .await
    .unwrap();

    // Lock the role, the way a concurrent rettighet change does.
    let mut laas = state.pool.begin().await.unwrap();
    sqlx::query("select id from company_role where id = $1 for update")
        .bind(role_id)
        .fetch_one(&mut *laas)
        .await
        .unwrap();

    // The change starts — and does not get past the lock.
    let pool = state.pool.clone();
    let skriver = tokio::spawn(async move {
        regnmed_db::roller::sett_rettigheter(
            &pool,
            company,
            role_id,
            &["RESKONTRO_LES".to_string()],
            som_skriver,
        )
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    for i in 0..20 {
        let sett = regnmed_db::roller::rettigheter_for(&state.pool, company, &navn)
            .await
            .unwrap();
        assert_eq!(
            sortert(sett),
            ["FAKTURA_LES", "FAKTURA_SKRIV"],
            "oppslag {i} så noe annet enn den gamle listen mens endringen pågikk"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    laas.commit().await.unwrap();
    skriver.await.unwrap().unwrap();
    let sett = regnmed_db::roller::rettigheter_for(&state.pool, company, &navn)
        .await
        .unwrap();
    assert_eq!(
        sortert(sett),
        ["RESKONTRO_LES"],
        "den nye listen skulle stått"
    );
}

/// The name is taken — and it must say precisely that. The unique
/// violation is recognised by SQLSTATE, not by the constraint name, so a
/// rename in a later migration cannot turn the message into a 500.
#[tokio::test]
async fn a_duplicate_role_name_says_the_name_is_taken() {
    let Some((state, _idp, company, admin, _bokforing, _les)) = setup().await else {
        return;
    };
    let kropp = r#"{"navn":"Kontrollør","rettigheter":["FAKTURA_LES"]}"#;
    let (kode, svar) = json_call(
        &state,
        "POST",
        &format!("/companies/{company}/roles"),
        &admin,
        kropp,
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");

    let (kode, svar) = json_call(
        &state,
        "POST",
        &format!("/companies/{company}/roles"),
        &admin,
        kropp,
    )
    .await;
    assert_eq!(kode, StatusCode::BAD_REQUEST, "{svar}");
    assert!(
        svar["error"]
            .as_str()
            .unwrap_or_default()
            .contains("allerede en rolle"),
        "{svar}"
    );
}

/// Built-in names cannot be hijacked — a custom "admin" would shadow the
/// real one.
#[tokio::test]
async fn built_in_role_names_are_reserved() {
    let Some((state, _idp, company, admin, _bokforing, _les)) = setup().await else {
        return;
    };
    for navn in ["admin", "les", "bokforing", "ansatt", "revisor"] {
        let (kode, svar) = json_call(
            &state,
            "POST",
            &format!("/companies/{company}/roles"),
            &admin,
            &format!(r#"{{"navn":"{navn}","rettigheter":["FAKTURA_LES"]}}"#),
        )
        .await;
        assert_eq!(kode, StatusCode::BAD_REQUEST, "{navn}: {svar}");
    }
}
