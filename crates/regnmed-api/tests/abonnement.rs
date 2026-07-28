//! Abonnementet (#65, docs/abonnement.md): status og sperre.
//!
//! Det viktigste her er PRINSIPPET, testet som nektelser og
//! IKKE-nektelser om hverandre: et sperret abonnement stopper endringer
//! — og INGENTING annet. Lesing, eksport og styringen av selskapet skal
//! bevises å virke på et sperret selskap, ellers er «hovedboken tas
//! aldri som gissel» bare en setning i dokumentasjonen.
//!
//! Krever DATABASE_URL; hopper over ellers.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Datelike, Utc};
use common::{TestIdp, test_state, unique_orgnr};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn kall(
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
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (
        kode,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

async fn oppsett() -> Option<(AppState, TestIdp, Uuid, String)> {
    let idp = TestIdp::new();
    let state = test_state(&idp).await?;
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Abonnementstest AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    let sub = format!("admin|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Admin"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let token = idp.token(&sub, "Admin");
    Some((state, idp, company, token))
}

/// Setter selskapet i en gitt fase ved å skru på klokken og dekningen.
async fn gjor_sperret(state: &AppState, company: Uuid) {
    // Selskapet «ble opprettet» for lengst, og har ingen dekning:
    // prøvetid og frist er begge ute.
    sqlx::query("update company set created_at = now() - interval '120 days' where id = $1")
        .bind(company)
        .execute(&state.pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn nytt_selskap_er_i_provetid_og_kan_alt() {
    let Some((state, _idp, company, admin)) = oppsett().await else {
        return;
    };
    let (kode, _) = kall(
        &state,
        "POST",
        &format!("/companies/{company}/products"),
        &admin,
        r#"{"nummer":"1","navn":"Vare","salgspris_ore":1000}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "prøvetiden skal virke som normalt");

    let (_, me) = kall(&state, "GET", "/me", &admin, "").await;
    let selskap = me["companies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["company_id"] == company.to_string())
        .expect("selskapet i /me");
    assert_eq!(selskap["abonnement"]["status"], "prove");
    assert!(
        selskap["abonnement"]["dato"].is_string(),
        "prøvetiden skal si når den løper ut"
    );
}

/// Kjernen: sperret stopper endringer — og ingenting annet.
#[tokio::test]
async fn sperret_stopper_endringer_og_ingenting_annet() {
    let Some((state, _idp, company, admin)) = oppsett().await else {
        return;
    };
    gjor_sperret(&state, company).await;

    // Endringer: nektet, med forklaringen i svaret.
    let (kode, svar) = kall(
        &state,
        "POST",
        &format!("/companies/{company}/products"),
        &admin,
        r#"{"nummer":"1","navn":"Vare","salgspris_ore":1000}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::FORBIDDEN);
    assert!(
        svar["error"]
            .as_str()
            .unwrap_or_default()
            .contains("abonnement"),
        "avvisningen skal forklare seg: {svar}"
    );

    // Lesing: virker.
    let (kode, _) = kall(
        &state,
        "GET",
        &format!("/companies/{company}/invoices"),
        &admin,
        "",
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "lesing skal alltid virke");

    // Eksport: virker — hovedboken tas aldri som gissel.
    let year = Utc::now().year();
    let (kode, _) = kall(
        &state,
        "GET",
        &format!(
            "/companies/{company}/reports/saft?year={year}&contact_first=Test&contact_last=Person"
        ),
        &admin,
        "",
    )
    .await;
    assert_ne!(
        kode,
        StatusCode::FORBIDDEN,
        "SAF-T-eksport skal virke på et sperret selskap"
    );

    // Styringen av selskapet: virker, ellers kunne ingen ryddet opp.
    let (kode, _) = kall(
        &state,
        "GET",
        &format!("/companies/{company}/access"),
        &admin,
        "",
    )
    .await;
    assert_eq!(kode, StatusCode::OK);
    let (kode, _) = kall(
        &state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &admin,
        r#"{"epost":"hjelp@byra.no","rolle":"bokforing"}"#,
    )
    .await;
    assert_eq!(
        kode,
        StatusCode::OK,
        "et sperret selskap må kunne slippe inn den som skal ordne opp"
    );

    // /me sier det som det er.
    let (_, me) = kall(&state, "GET", "/me", &admin, "").await;
    let selskap = me["companies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["company_id"] == company.to_string())
        .unwrap();
    assert_eq!(selskap["abonnement"]["status"], "sperret");
}

/// Tegning åpner igjen, oppsigelse gir frist — ikke øyeblikkelig sperre.
#[tokio::test]
async fn tegning_apner_og_oppsigelse_gir_frist() {
    let Some((state, _idp, company, admin)) = oppsett().await else {
        return;
    };
    gjor_sperret(&state, company).await;

    let idag = Utc::now().date_naive();
    regnmed_db::abonnement::tegn(
        &state.pool,
        company,
        "standard",
        idag,
        None,
        "test: tegning etter sperre",
        "test",
    )
    .await
    .unwrap();

    let (kode, _) = kall(
        &state,
        "POST",
        &format!("/companies/{company}/products"),
        &admin,
        r#"{"nummer":"2","navn":"Vare to","salgspris_ore":1000}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "tegning skal åpne skrivingen igjen");

    // Oppsigelse fra i morgen: dekningen står ut dagen, deretter løper
    // fristen — endringer virker fortsatt, banneret varsler.
    regnmed_db::abonnement::avslutt(&state.pool, company, idag + chrono::Days::new(1))
        .await
        .unwrap();
    let status = regnmed_db::abonnement::status_for(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(status.slug(), "aktiv", "dekningen står ut siste dag");
}

/// Fakturakjøringen: regnmed fakturerer seg selv med sin egen motor —
/// og en måned kan ikke faktureres to ganger.
#[tokio::test]
async fn abonnementsfaktura_gjennom_egen_motor_er_idempotent() {
    let Some((state, _idp, kunde, _admin)) = oppsett().await else {
        return;
    };
    let drift = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Regnmed Drift AS")
        .await
        .unwrap();
    let idag = Utc::now().date_naive();
    regnmed_db::abonnement::tegn(
        &state.pool,
        kunde,
        "standard",
        NaiveDateHelper::forste_i_maneden(idag),
        None,
        "test: dekning for fakturering",
        "test",
    )
    .await
    .unwrap();

    // `bare`-avgrensningen: dev-databasen kan ha tusenvis av selskaper
    // med dekning (migrasjonens såing), og testen skal ikke fakturere
    // dem alle.
    let forste = regnmed_db::abonnement::fakturer_maned(&state.pool, drift, idag, Some(kunde))
        .await
        .unwrap();
    let vare = forste
        .iter()
        .find(|u| u.company_id == kunde)
        .expect("kundeselskapet i utfallet");
    let nr = match vare.invoice_no {
        Some(n) => n,
        None => panic!("faktura skal være utstedt, men: {:?}", vare.detail),
    };

    // Kjøring nummer to: samme måned, ingen ny faktura.
    let andre = regnmed_db::abonnement::fakturer_maned(&state.pool, drift, idag, Some(kunde))
        .await
        .unwrap();
    let vare2 = andre.iter().find(|u| u.company_id == kunde).unwrap();
    assert!(
        vare2.invoice_no.is_none(),
        "måneden skal ikke kunne faktureres to ganger (første var {nr})"
    );

    // Fakturaen bor i DRIFTSSELSKAPETS hovedbok, med kundens orgnr på
    // parten — beløpet er prislistens.
    let (belop, orgnr): (i64, String) = sqlx::query_as(
        "select (select sum(l.net_ore)::bigint
                   from invoice_line l where l.invoice_id = i.id),
                p.orgnr
         from invoice i
         join party p on p.id = i.party_id
         where i.company_id = $1 and p.orgnr = $2
         order by i.invoice_no desc limit 1",
    )
    .bind(drift)
    .bind(
        sqlx::query_scalar::<_, String>("select orgnr from company where id = $1")
            .bind(kunde)
            .fetch_one(&state.pool)
            .await
            .unwrap(),
    )
    .fetch_one(&state.pool)
    .await
    .unwrap();
    let pris = regnmed_db::abonnement::pris_pa(&state.pool, "standard", idag)
        .await
        .unwrap();
    assert_eq!(belop, pris);
    let kunde_orgnr: String = sqlx::query_scalar("select orgnr from company where id = $1")
        .bind(kunde)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(orgnr, kunde_orgnr);
}

struct NaiveDateHelper;
impl NaiveDateHelper {
    fn forste_i_maneden(d: chrono::NaiveDate) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap()
    }
}
