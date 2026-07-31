//! The abonnement (#65, docs/abonnement.md): status and blocking.
//!
//! What matters here is the PRINCIPLE, tested as refusals and
//! NON-refusals side by side: a blocked abonnement stops changes — and
//! NOTHING else. Reading, export and governance of the company must be
//! proven to work on a blocked company, otherwise "the hovedbok is never
//! taken hostage" is just a sentence in the documentation.
//!
//! Requires DATABASE_URL; skips otherwise.

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

/// Puts the company in a given phase by moving the clock and the coverage.
async fn make_blocked(state: &AppState, company: Uuid) {
    // The company "was created" long ago and has no coverage: both the
    // trial and the deadline are over.
    sqlx::query("update company set created_at = now() - interval '120 days' where id = $1")
        .bind(company)
        .execute(&state.pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_new_company_is_in_its_trial_and_can_do_everything() {
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

/// The core: blocked stops changes — and nothing else.
#[tokio::test]
async fn a_block_stops_changes_and_nothing_else() {
    let Some((state, _idp, company, admin)) = oppsett().await else {
        return;
    };
    make_blocked(&state, company).await;

    // Changes: refused, with the explanation in the response.
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

    // Export: works — the hovedbok is never taken hostage.
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

    // Governance of the company: works, or nobody could sort things out.
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

    // /me says it as it is.
    let (_, me) = kall(&state, "GET", "/me", &admin, "").await;
    let selskap = me["companies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["company_id"] == company.to_string())
        .unwrap();
    assert_eq!(selskap["abonnement"]["status"], "sperret");
}

/// Signing up reopens, cancelling gives a deadline — not an instant block.
#[tokio::test]
async fn signing_up_opens_and_cancelling_gives_a_deadline() {
    let Some((state, _idp, company, admin)) = oppsett().await else {
        return;
    };
    make_blocked(&state, company).await;

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

/// The invoicing run: regnmed invoices itself with its own engine — and a
/// month cannot be invoiced twice.
#[tokio::test]
async fn the_abonnement_faktura_through_our_own_engine_is_idempotent() {
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

    // The `bare` narrowing: the dev database can hold thousands of
    // companies with coverage (the migration's seeding), and the test
    // must not invoice them all.
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

    // The faktura lives in the OPS COMPANY's hovedbok, with the
    // customer's orgnr on the party — the amount is the price list's.
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
                .uri("/webhooks/stripe")
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

fn with_stripe(base: &AppState, drift_orgnr: &str) -> AppState {
    AppState {
        stripe: Some(regnmed_api::StripeCfg {
            secret_key: "sk_test_x".into(),
            webhook_secret: "whsec_test".into(),
            api_base: None,
        }),
        drift_orgnr: Some(drift_orgnr.into()),
        portal_base: None,
        ..base.clone()
    }
}

/// The webhook is authenticated by the signature — without a valid one
/// NOTHING happens, however correct the payload looks.
#[tokio::test]
async fn the_webhook_rejects_an_invalid_signature() {
    let Some((state, _idp, _company, _admin)) = oppsett().await else {
        return;
    };
    let state = with_stripe(&state, "999999999");
    let payload = br#"{"type":"payment_intent.succeeded","data":{"object":{}}}"#;
    let (kode, _) = webhook_post(&state, payload, "t=1,v1=feil").await;
    assert_eq!(kode, StatusCode::BAD_REQUEST);
}

/// A Stripe account can serve more than one product, so sessions that were
/// never ours arrive here as well — and our OWN subscription sessions carry
/// no setup_intent. Both must be acknowledged. A 4xx would make Stripe
/// retry for three days and then disable the endpoint, which would silence
/// the events that ARE ours.
#[tokio::test]
async fn a_session_that_carries_no_card_is_acknowledged_not_rejected() {
    let Some((state, _idp, kunde, _admin)) = oppsett().await else {
        return;
    };
    let state = with_stripe(&state, "999999999");
    let send = |event: serde_json::Value| {
        let state = state.clone();
        async move {
            let payload = serde_json::to_vec(&event).unwrap();
            let sig =
                regnmed_gov::stripe::sign_webhook(&payload, "whsec_test", Utc::now().timestamp());
            webhook_post(&state, &payload, &sig).await
        }
    };

    // Another product's checkout in the same Stripe account: no reference
    // of ours, so nothing to do — but it must still be a 200.
    let (kode, svar) = send(serde_json::json!({
        "type": "checkout.session.completed",
        "data": { "object": { "id": "cs_someone_else", "mode": "payment" } }
    }))
    .await;
    assert_eq!(kode, StatusCode::OK);
    assert_eq!(svar["handled"], "ignorert");

    // Our own subscription session: the card is attached by Stripe, and the
    // subscription reaches us on customer.subscription.created instead.
    let (kode, svar) = send(serde_json::json!({
        "type": "checkout.session.completed",
        "data": { "object": {
            "id": "cs_abonnement",
            "mode": "subscription",
            "client_reference_id": kunde.to_string(),
            "customer": "cus_test",
            "subscription": "sub_test"
        } }
    }))
    .await;
    assert_eq!(kode, StatusCode::OK);
    assert_eq!(svar["handled"], "abonnement_sesjon");
}

/// The whole card path: an abonnement faktura is issued, the webhook
/// confirms the charge — the payment bilag is posted in the ops company
/// and the reskontro item is closed. And a REPLAY of the same event posts
/// nothing: uniqueness on payment_intent is the dedup key.
#[tokio::test]
async fn a_card_charge_posts_once_and_only_once() {
    let Some((state, _idp, kunde, _admin)) = oppsett().await else {
        return;
    };
    let drift_orgnr = unique_orgnr();
    let drift = regnmed_db::create_company(&state.pool, &drift_orgnr, "Regnmed Drift AS")
        .await
        .unwrap();
    let state = with_stripe(&state, &drift_orgnr);

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

    // The reskontro item is closed: the receivable has zero remaining.
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

    // Replay: the same event again — acknowledged, but posts nothing.
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

/// Card-first: with the card rail on, the abonnement cannot be started
/// self-service without a card — and the error says what is missing.
/// Signing up, on both rails.
///
/// WITHOUT the card rail (self-hosted, or invoicing agreed with ops) the
/// coverage row is written directly and the company is active at once.
///
/// WITH the rail on, signup goes through Stripe Checkout in subscription
/// mode instead — so the assertion that used to live here, that signup is
/// refused until a card exists, is gone on purpose: Checkout collects the
/// card as part of subscribing, and demanding one first would be two
/// steps where Stripe offers one. What must still hold is that a company
/// cannot end up with two live subscriptions, and that check runs BEFORE
/// any call goes out to Stripe — which is also why it is testable here.
#[tokio::test]
async fn signing_up_activates_directly_without_the_card_rail() {
    let Some((state, _idp, company, admin)) = oppsett().await else {
        return;
    };
    // Rail off: `state` from oppsett() has no StripeCfg.
    let (kode, svar) = kall(
        &state,
        "POST",
        &format!("/companies/{company}/subscription"),
        &admin,
        r#"{"plan":"standard"}"#,
    )
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    assert_eq!(svar["status"], "aktiv");
    assert_eq!(
        regnmed_db::abonnement::status_for(&state.pool, company)
            .await
            .unwrap()
            .slug(),
        "aktiv"
    );
}

/// One live subscription per company. The guard sits before any Stripe
/// call, so a double click cannot start a second one that would then bill
/// the customer twice every month until somebody noticed.
#[tokio::test]
async fn a_company_cannot_start_a_second_subscription() {
    let Some((state, _idp, company, admin)) = oppsett().await else {
        return;
    };
    let state = with_stripe(&state, "999999999");
    regnmed_db::abonnement::lagre_stripe_abo(
        &state.pool,
        company,
        &format!("sub_{}", Uuid::new_v4()),
        "price_x",
        "standard",
        "month",
        "active",
        "test",
    )
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
    assert_eq!(kode, StatusCode::BAD_REQUEST, "{svar}");
    assert!(
        svar["error"]
            .as_str()
            .unwrap_or_default()
            .contains("løpende abonnement"),
        "{svar}"
    );
}

/// A Stripe subscription recurs until it is cancelled — the whole point
/// of moving the abonnement our customers pay US for onto Stripe.
///
/// Driven entirely through webhooks, which is honest: no call goes OUT to
/// Stripe here, so the test proves our side of the contract without a
/// mock pretending to be theirs.
///
/// What must hold, in order:
///   1. `customer.subscription.created` opens coverage — the customer is
///      active because Stripe says the subscription exists.
///   2. `invoice.paid` books it OUR way: a faktura through the ordinary
///      engine, the payment against it, and the reskontro closed — so the
///      sale is in the drift company's hovedbok as bokføringsloven wants,
///      whoever collected the money.
///   3. A REDELIVERED `invoice.paid` books NOTHING. Stripe retries
///      webhooks, so this is not hypothetical.
///   4. `customer.subscription.deleted` closes the coverage.
#[tokio::test]
async fn a_stripe_subscription_recurs_until_cancelled() {
    let Some((state, _idp, kunde, _admin)) = oppsett().await else {
        return;
    };
    let drift_orgnr = unique_orgnr();
    let drift = regnmed_db::create_company(&state.pool, &drift_orgnr, "Regnmed Drift AS")
        .await
        .unwrap();
    let state = with_stripe(&state, &drift_orgnr);
    let sub_id = format!("sub_test_{}", Uuid::new_v4());

    let send = async |body: serde_json::Value| -> (StatusCode, serde_json::Value) {
        let payload = serde_json::to_vec(&body).unwrap();
        let sig = regnmed_gov::stripe::sign_webhook(&payload, "whsec_test", Utc::now().timestamp());
        webhook_post(&state, &payload, &sig).await
    };

    // 1. Subscription created → coverage opens.
    let (kode, svar) = send(serde_json::json!({
        "type": "customer.subscription.created",
        "data": { "object": {
            "id": sub_id,
            "status": "active",
            "metadata": { "regnmed_company_id": kunde.to_string() },
            "items": { "data": [ { "price": {
                "id": "price_test", "recurring": { "interval": "month" },
                "metadata": { "regnmed_plan": "standard" }
            } } ] },
        }}
    }))
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    assert_eq!(svar["handled"], "abonnement_opprettet");
    assert_eq!(
        regnmed_db::abonnement::status_for(&state.pool, kunde)
            .await
            .unwrap()
            .slug(),
        "aktiv",
        "et opprettet Stripe-abonnement skal gi dekning"
    );

    // 2. First payment → faktura + payment + closed reskontro, in OUR books.
    let stripe_invoice = format!("in_test_{}", Uuid::new_v4());
    // Shaped like a REAL delivery from Stripe API 2026-05-27.dahlia: the
    // subscription sits under parent.subscription_details, and there is no
    // payment_intent. Written from a captured live event, not from what
    // the handler happens to read — the previous version of this payload
    // carried `subscription` at the top level, so it passed against code
    // that could not bill anyone.
    let betalt = serde_json::json!({
        "type": "invoice.paid",
        "data": { "object": {
            "id": stripe_invoice,
            "parent": { "subscription_details": { "subscription": sub_id } },
            "amount_paid": 12_375i64,   // 99 kr + 25 % mva
            "lines": { "data": [ { "description": "regnmed standard — august 2026" } ] },
        }}
    });
    let (kode, svar) = send(betalt.clone()).await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    assert_eq!(svar["handled"], "bokfort");

    let (fakturaer, rest): (i64, i64) = sqlx::query_as(
        "select count(*)::bigint,
                coalesce(sum(e.amount_ore
                    - coalesce((select sum(m.amount_ore) from reskontro_match m
                                 where m.entry_a = e.id), 0)), 0)::bigint
         from invoice i join entry e on e.id = i.receivable_entry_id
         where i.company_id = $1",
    )
    .bind(drift)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(fakturaer, 1, "betalingen skulle gitt én faktura");
    assert_eq!(rest, 0, "fakturaen skulle vært lukket av betalingen");

    // 3. Stripe redelivers the same event — nothing is booked twice.
    let (kode, svar) = send(betalt).await;
    assert_eq!(kode, StatusCode::OK);
    assert_eq!(svar["handled"], "replay", "en replay skal ikke bokføre");
    let fakturaer: i64 = sqlx::query_scalar("select count(*) from invoice where company_id = $1")
        .bind(drift)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(fakturaer, 1, "replay skulle ikke gitt en ny faktura");

    // 4. Cancelled → coverage closes.
    let (kode, svar) = send(serde_json::json!({
        "type": "customer.subscription.deleted",
        "data": { "object": { "id": sub_id, "status": "canceled" } }
    }))
    .await;
    assert_eq!(kode, StatusCode::OK, "{svar}");
    assert_eq!(svar["handled"], "abonnement_avsluttet");
    assert!(
        regnmed_db::abonnement::stripe_abo_for(&state.pool, kunde)
            .await
            .unwrap()
            .is_none(),
        "et oppsagt abonnement skal ikke lenger regnes som løpende"
    );
}
