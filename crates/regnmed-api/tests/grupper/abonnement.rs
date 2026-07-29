//! Abonnementet (#65, docs/abonnement.md): status og sperre.
//!
//! Det viktigste her er PRINSIPPET, testet som nektelser og
//! IKKE-nektelser om hverandre: et sperret abonnement stopper endringer
//! — og INGENTING annet. Lesing, eksport og styringen av selskapet skal
//! bevises å virke på et sperret selskap, ellers er «hovedboken tas
//! aldri som gissel» bare en setning i dokumentasjonen.
//!
//! Krever DATABASE_URL; hopper over ellers.

use crate::common::{TestIdp, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Datelike, Utc};
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

// ---------------------------------------------------------------------
// Kortskinnen (#74).
// ---------------------------------------------------------------------

async fn webhook_post(
    state: &AppState,
    body: &[u8],
    signature: &str,
) -> (StatusCode, serde_json::Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/stripe/webhook")
                .header("stripe-signature", signature)
                .header("content-type", "application/json")
                .body(Body::from(body.to_vec()))
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

fn med_stripe(base: &AppState, drift_orgnr: &str) -> AppState {
    AppState {
        stripe: Some(regnmed_api::StripeCfg {
            secret_key: "sk_test_x".into(),
            webhook_secret: "whsec_test".into(),
            api_base: None,
        }),
        drift_orgnr: Some(drift_orgnr.into()),
        ..base.clone()
    }
}

/// Webhooken er autentisert av signaturen — uten gyldig signatur skjer
/// INGENTING, uansett hvor riktig payloaden ser ut.
#[tokio::test]
async fn webhook_avviser_ugyldig_signatur() {
    let Some((state, _idp, _company, _admin)) = oppsett().await else {
        return;
    };
    let state = med_stripe(&state, "999999999");
    let payload = br#"{"type":"payment_intent.succeeded","data":{"object":{}}}"#;
    let (kode, _) = webhook_post(&state, payload, "t=1,v1=feil").await;
    assert_eq!(kode, StatusCode::BAD_REQUEST);
}

/// Hele kortveien: abonnementsfaktura utstedes, webhooken bekrefter
/// trekket — betalingsbilag bokføres i driftsselskapet og
/// reskontroposten lukkes. Og en REPLAY av samme hendelse bokfører
/// ingenting: unikheten på payment_intent er dedup-nøkkelen.
#[tokio::test]
async fn korttrekk_bokfores_en_gang_og_bare_en() {
    let Some((state, _idp, kunde, _admin)) = oppsett().await else {
        return;
    };
    let drift_orgnr = unique_orgnr();
    let drift = regnmed_db::create_company(&state.pool, &drift_orgnr, "Regnmed Drift AS")
        .await
        .unwrap();
    let state = med_stripe(&state, &drift_orgnr);

    let idag = Utc::now().date_naive();
    regnmed_db::abonnement::tegn(
        &state.pool,
        kunde,
        "standard",
        NaiveDateHelper::forste_i_maneden(idag),
        None,
        "test: kortvei",
        "test",
    )
    .await
    .unwrap();
    let utfall = regnmed_db::abonnement::fakturer_maned(&state.pool, drift, idag, Some(kunde))
        .await
        .unwrap();
    let faktura = utfall.iter().find(|u| u.company_id == kunde).unwrap();
    let invoice_id = faktura.invoice_id.expect("faktura utstedt");
    let gross = faktura.gross_ore.unwrap();

    let payload = serde_json::to_vec(&serde_json::json!({
        "type": "payment_intent.succeeded",
        "data": { "object": {
            "id": format!("pi_test_{invoice_id}"),
            "amount_received": gross,
            "metadata": { "invoice_id": invoice_id.to_string() },
        }}
    }))
    .unwrap();
    let now = Utc::now().timestamp();
    let signatur = regnmed_gov::stripe::sign_webhook(&payload, "whsec_test", now);

    let (kode, svar) = webhook_post(&state, &payload, &signatur).await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    assert_eq!(svar["handled"], "bokfort");

    // Reskontroposten er lukket: fordringen har null i rest.
    let rest: i64 = sqlx::query_scalar(
        "select (e.amount_ore
                 - coalesce((select sum(m.amount_ore) from reskontro_match m
                              where m.entry_a = e.id), 0))::bigint
         from invoice i join entry e on e.id = i.receivable_entry_id
         where i.id = $1",
    )
    .bind(invoice_id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(rest, 0, "fordringen skulle vært lukket av korttrekket");

    // Replay: samme hendelse igjen — kvitteres, men bokfører ingenting.
    let (kode, svar) = webhook_post(&state, &payload, &signatur).await;
    assert_eq!(kode, StatusCode::OK);
    assert_eq!(svar["handled"], "replay");
    let bilag: i64 = sqlx::query_scalar(
        "select count(*) from voucher
         where company_id = $1 and description like 'Kortbetaling%'",
    )
    .bind(drift)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(bilag, 1, "replay skulle ikke gitt et nytt bilag");
}

/// Kort-først: med kortskinnen på kan abonnementet ikke startes
/// selvbetjent uten kort — og feilmeldingen sier hva som mangler.
#[tokio::test]
async fn selvbetjent_tegning_krever_kort_nar_skinnen_er_pa() {
    let Some((state, _idp, company, admin)) = oppsett().await else {
        return;
    };
    let state = med_stripe(&state, "999999999");
    let (kode, svar) = kall(
        &state,
        "POST",
        &format!("/companies/{company}/subscription"),
        &admin,
        r#"{"plan":"standard"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::BAD_REQUEST, "{svar}");
    assert!(
        svar["error"]
            .as_str()
            .unwrap_or_default()
            .contains("betalingskort"),
        "{svar}"
    );

    // Med kort «på plass» (lagret direkte — checkout-flyten er Stripes)
    // går tegningen, og statusen blir aktiv.
    regnmed_db::abonnement::lagre_kort(&state.pool, company, "cus_x", "pm_x", "visa", "4242")
        .await
        .unwrap();
    let (kode, svar) = kall(
        &state,
        "POST",
        &format!("/companies/{company}/subscription"),
        &admin,
        r#"{"plan":"standard"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    assert_eq!(
        regnmed_db::abonnement::status_for(&state.pool, company)
            .await
            .unwrap()
            .slug(),
        "aktiv"
    );
}
