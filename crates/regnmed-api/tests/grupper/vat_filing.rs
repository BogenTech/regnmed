//! «Godkjenn og send» for the mva-melding (docs/mva.md), against a mock of
//! ID-porten and Altinn. Requires DATABASE_URL; skips otherwise.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{Request, StatusCode};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use regnmed_core::Ore;
use regnmed_core::secret::SealingKey;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use regnmed_db::job;
use regnmed_gov::idporten::IdportenClient;
use regnmed_gov::vat_return::{VatFilingClient, VatReturnClient};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;
use tower::ServiceExt;
use uuid::Uuid;

use crate::common::{KID, TestIdp, make_invoiceable, test_state, unique_orgnumber};
use regnmed_api::{AppState, IdportenRail, router};

const PID: &str = "01010112345";
const ID_TOKEN: &str = "idporten-access-token-secret";
const ALTINN_TOKEN: &str = "altinn-token-secret";
const CLEAN: &str = "<valideringsresultat><avvikVedMeldingslevering>ingen avvik</avvikVedMeldingslevering></valideringsresultat>";
const REFUSED: &str = "<valideringsresultat><avvikVedMeldingslevering>ugyldig skattemelding</avvikVedMeldingslevering><avvik><stiTilAvvik>//x</stiTilAvvik><avviksinformasjon><begrunnelse>Summen stemmer ikke</begrunnelse></avviksinformasjon></avvik></valideringsresultat>";
const PAYMENT: &str = "<betalingsinformasjon><kundeidentifikasjonsnummer>12345678903</kundeidentifikasjonsnummer><beloep>2500</beloep><betalingsfrist>2026-04-10</betalingsfrist><betalesTilKonto><norskKontonummer>12345678903</norskKontonummer></betalesTilKonto></betalingsinformasjon>";

#[derive(Default)]
struct Mock {
    base: String,
    nonce: String,
    challenge: String,
    wrong_nonce: bool,
    expires_in: Option<i64>,
    validation: Option<&'static str>,
    create_fails: Option<u16>,
    next_fails_once: Option<(usize, u16, bool)>,
    creates: usize,
    nexts: usize,
    melding_uploads: usize,
    task: usize,
    feedback: bool,
    signing_key: Option<EncodingKey>,
    sub: Option<&'static str>,
    auth_time_offset: i64,
    exchange_fails: Option<u16>,
    after_confirm: AfterConfirm,
    no_receipt: bool,
    result: Option<&'static str>,
    deletes: usize,
    delete_fails: bool,
    read_fails_once: Option<u16>,
    gates: HashMap<&'static str, Gate>,
    /// Who Altinn says signed in. `None` is the default test person.
    signer: Option<&'static str>,
    profile_fails: bool,
    envelope: Option<String>,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum AfterConfirm {
    #[default]
    Feedback,
    Ended,
    UnknownTask,
    NoProcess,
}

/// Holds a call open: `.0` says it arrived, `.1` lets it go on.
type Gate = (Arc<Notify>, Arc<Notify>);

async fn pass(m: &Shared, name: &str) {
    let gate = m.lock().unwrap().gates.remove(name);
    if let Some((reached, release)) = gate {
        reached.notify_one();
        release.notified().await;
    }
}

fn gate(s: &Setup, name: &'static str) -> Gate {
    let gate = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    s.mock.lock().unwrap().gates.insert(name, gate.clone());
    gate
}

type Shared = Arc<Mutex<Mock>>;

fn instance(mock: &Mock) -> Value {
    let task = match mock.task {
        0 => json!({ "elementId": "Task_1" }),
        1 => json!({ "elementId": "Task_2" }),
        _ if mock.feedback || mock.after_confirm == AfterConfirm::Ended => Value::Null,
        _ if mock.after_confirm == AfterConfirm::UnknownTask => {
            json!({ "elementId": "Task_9", "name": "Noe annet" })
        }
        _ => json!({ "elementId": "Task_3", "name": "Tilbakemelding" }),
    };
    let mut data = vec![json!({
        "id": "envelope",
        "dataType": regnmed_gov::vat_return::ENVELOPE_DATA_TYPE,
    })];
    if mock.melding_uploads > 0 {
        data.push(json!({ "id": "melding", "dataType": "mvamelding" }));
    }
    if mock.task >= 2 {
        data.push(json!({
            "id": "payment", "dataType": "betalingsinformasjon", "contentType": "text/xml",
        }));
    }
    if mock.feedback {
        data.push(json!({
            "id": "result", "dataType": "valideringsresultat", "contentType": "text/xml",
        }));
        if !mock.no_receipt {
            data.push(json!({
                "id": "receipt", "dataType": "kvittering", "contentType": "application/pdf",
            }));
        }
    }
    let ended = (mock.task >= 2 && (mock.feedback || mock.after_confirm == AfterConfirm::Ended))
        .then_some("2026-03-01T10:00:00Z");
    let mut body = json!({ "id": "5000/guid", "data": data });
    if !(mock.task >= 2 && mock.after_confirm == AfterConfirm::NoProcess) {
        body["process"] = json!({ "currentTask": task, "ended": ended });
    }
    body
}

async fn mock_server(idp: &TestIdp) -> Shared {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mock: Shared = Arc::new(Mutex::new(Mock {
        base: base.clone(),
        signing_key: Some(idp.encoding_key.clone()),
        ..Default::default()
    }));
    let jwks = idp.jwks.clone();
    let app = Router::new()
        .route(
            "/.well-known/openid-configuration",
            get(|State(m): State<Shared>| async move {
                let base = m.lock().unwrap().base.clone();
                Json(json!({
                    "issuer": base,
                    "authorization_endpoint": format!("{base}/authorize"),
                    "token_endpoint": format!("{base}/token"),
                    "jwks_uri": format!("{base}/jwks"),
                }))
            }),
        )
        .route(
            "/jwks",
            get(move || {
                let jwks = jwks.clone();
                async move { Json(jwks) }
            }),
        )
        .route(
            "/token",
            post(|State(m): State<Shared>, body: String| async move {
                let answer = {
                    let m = m.lock().unwrap();
                    let form: std::collections::HashMap<String, String> =
                        reqwest::Url::parse(&format!("http://form.invalid/?{body}"))
                            .unwrap()
                            .query_pairs()
                            .into_owned()
                            .collect();
                    let digest = Sha256::digest(form["code_verifier"].as_bytes());
                    if URL_SAFE_NO_PAD.encode(digest) != m.challenge {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": "invalid_grant" })),
                        );
                    }
                    let now = chrono::Utc::now().timestamp();
                    let claims = json!({
                        "iss": m.base, "aud": "client", "sub": m.sub.unwrap_or("pairwise-sub"),
                        "acr": "idporten-loa-high", "auth_time": now + m.auth_time_offset,
                        "exp": now + 120,
                        "nonce": if m.wrong_nonce { "another" } else { m.nonce.as_str() },
                        "pid": PID,
                    });
                    let mut header = Header::new(Algorithm::RS256);
                    header.kid = Some(KID.into());
                    let id_token =
                        encode(&header, &claims, m.signing_key.as_ref().unwrap()).unwrap();
                    json!({
                        "access_token": ID_TOKEN,
                        "id_token": id_token,
                        "expires_in": m.expires_in.unwrap_or(120),
                    })
                };
                pass(&m, "token").await;
                (StatusCode::OK, Json(answer))
            }),
        )
        .route(
            "/valider",
            post(|State(m): State<Shared>| async move {
                let answer = m.lock().unwrap().validation.unwrap_or(CLEAN).to_string();
                pass(&m, "valider").await;
                answer
            }),
        )
        .route(
            "/authentication/api/v1/exchange/id-porten",
            get(|State(m): State<Shared>| async move {
                match m.lock().unwrap().exchange_fails {
                    Some(status) => (StatusCode::from_u16(status).unwrap(), String::new()),
                    None => (StatusCode::OK, format!("\"{ALTINN_TOKEN}\"")),
                }
            }),
        )
        .route(
            "/api/v1/profile/user",
            get(|State(m): State<Shared>| async move {
                let m = m.lock().unwrap();
                if m.profile_fails {
                    return (StatusCode::BAD_GATEWAY, Json(json!({})));
                }
                let name = m.signer.unwrap_or("Kari Nordmann");
                (
                    StatusCode::OK,
                    Json(json!({ "party": { "name": name, "ssn": PID } })),
                )
            }),
        )
        .route(
            "/instances",
            post(|State(m): State<Shared>| async move {
                let answer = {
                    let mut m = m.lock().unwrap();
                    m.creates += 1;
                    if let Some(status) = m.create_fails {
                        return (StatusCode::from_u16(status).unwrap(), Json(json!({})));
                    }
                    m.task = 0;
                    m.melding_uploads = 0;
                    instance(&m)
                };
                pass(&m, "create").await;
                (StatusCode::CREATED, Json(answer))
            }),
        )
        .route(
            "/instances/{party}/{guid}",
            get(|State(m): State<Shared>| async move {
                let mut m = m.lock().unwrap();
                if let Some(status) = m.read_fails_once.take() {
                    return (StatusCode::from_u16(status).unwrap(), Json(json!({})));
                }
                (StatusCode::OK, Json(instance(&m)))
            })
            .delete(|State(m): State<Shared>| async move {
                    let mut m = m.lock().unwrap();
                    m.deletes += 1;
                    if m.delete_fails {
                        StatusCode::FORBIDDEN
                    } else {
                        StatusCode::OK
                    }
                }),
        )
        .route(
            "/instances/{party}/{guid}/data",
            post(|State(m): State<Shared>| async move {
                m.lock().unwrap().melding_uploads += 1;
                StatusCode::CREATED
            }),
        )
        .route(
            "/instances/{party}/{guid}/data/{id}",
            put(|State(m): State<Shared>, body: String| async move {
                m.lock().unwrap().envelope = Some(body);
                StatusCode::OK
            })
            .get(
                |State(m): State<Shared>, Path((_, _, id)): Path<(String, String, String)>| async move {
                    match id.as_str() {
                        "payment" => PAYMENT.to_string(),
                        "result" => m.lock().unwrap().result.unwrap_or(CLEAN).to_string(),
                        _ => "%PDF-receipt".to_string(),
                    }
                },
            ),
        )
        .route(
            "/instances/{party}/{guid}/process/next",
            put(|State(m): State<Shared>| async move {
                let answer = {
                    let mut m = m.lock().unwrap();
                    m.nexts += 1;
                    if let Some((at, status, moved)) = m.next_fails_once
                        && m.nexts == at
                    {
                        if moved {
                            m.task += 1;
                        }
                        m.next_fails_once = None;
                        StatusCode::from_u16(status).unwrap()
                    } else {
                        m.task += 1;
                        StatusCode::OK
                    }
                };
                pass(&m, "next").await;
                answer
            }),
        )
        .route(
            "/instances/{party}/{guid}/feedback/status",
            get(|State(m): State<Shared>| async move {
                Json(json!({ "isFeedbackProvided": m.lock().unwrap().feedback }))
            }),
        )
        .with_state(mock.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    mock
}

struct Setup {
    state: AppState,
    idp: TestIdp,
    mock: Shared,
    company: Uuid,
    token: String,
    worker: Arc<regnmed_db::vat_filing::Rail>,
}

fn sale(net: i64) -> VoucherDraft {
    let entry = |account: &str, amount: i64, vat: Option<&str>| EntryDraft {
        account_number: account.into(),
        amount: Ore(amount),
        vat_code: vat.map(str::to_string),
        description: None,
        party_no: None,
        department: None,
        project: None,
        currency: None,
    };
    VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
        description: "Salg".into(),
        reverses: None,
        entries: vec![
            entry("1920", net + net / 4, None),
            entry("3000", -net, Some("3")),
            entry("2700", -net / 4, None),
        ],
    }
}

async fn setup() -> Option<Setup> {
    let idp = TestIdp::new();
    let mut state = test_state(&idp).await?;
    let mock = mock_server(&idp).await;
    let base = mock.lock().unwrap().base.clone();
    let key = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let worker = Arc::new(regnmed_db::vat_filing::Rail {
        client: VatFilingClient::new(
            VatReturnClient::new(format!("{base}/valider")),
            base.clone(),
            format!("{base}/instances"),
        )
        .unwrap(),
        key: SealingKey::from_hex(key).unwrap(),
        lock_wait: Duration::from_millis(150),
    });
    state.idporten = Some(Arc::new(IdportenRail {
        client: IdportenClient::new(
            base,
            "client",
            "client-secret",
            "https://test.regnmed.invalid/mva-godkjenning",
        )
        .unwrap(),
        key: SealingKey::from_hex(key).unwrap(),
        local_worker: Some(worker.clone()),
    }));
    let orgnumber = std::iter::repeat_with(unique_orgnumber)
        .find(|n| regnmed_core::orgnumber::is_valid(n))
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &orgnumber, "Mva AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [("1920", "Bank"), ("3000", "Salg"), ("2700", "Utgående mva")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::post_voucher(&state.pool, company, &sale(10_000_00), "test")
        .await
        .unwrap();
    let token = member(&state, &idp, company, "admin").await;
    let (status, body) = call(
        &state,
        &token,
        "POST",
        &format!("/companies/{company}/terms/accept"),
        json!({ "versjon": *regnmed_api::terms::VERSION }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    Some(Setup {
        state,
        idp,
        mock,
        company,
        token,
        worker,
    })
}

async fn member(state: &AppState, idp: &TestIdp, company: Uuid, role: &str) -> String {
    named_member(state, idp, company, role, "Kari Nordmann").await
}

async fn named_member(
    state: &AppState,
    idp: &TestIdp,
    company: Uuid,
    role: &str,
    name: &str,
) -> String {
    let sub = format!("user|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some(name), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, role)
        .await
        .unwrap();
    idp.token(&sub, name)
}

async fn call(
    state: &AppState,
    token: &str,
    method: &str,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Where the portal would send the person, as the mock remembers it.
fn sign_in(s: &Setup, authorize_url: &str) -> String {
    let url = reqwest::Url::parse(authorize_url).unwrap();
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
            .unwrap()
    };
    assert_eq!(param("acr_values"), "idporten-loa-high");
    assert_eq!(param("code_challenge_method"), "S256");
    assert_eq!(
        param("prompt"),
        "login",
        "an old session is not an approval"
    );
    let mut m = s.mock.lock().unwrap();
    m.nonce = param("nonce");
    m.challenge = param("code_challenge");
    param("state")
}

async fn start(s: &Setup) -> (String, String) {
    let (status, body) = call(
        &s.state,
        &s.token,
        "POST",
        &format!("/companies/{}/vat-filings", s.company),
        json!({ "year": 2026, "period": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let login = sign_in(s, body["authorize_url"].as_str().unwrap());
    (body["filing"]["id"].as_str().unwrap().to_string(), login)
}

async fn approve(s: &Setup, token: &str, id: &str, login: &str) -> (StatusCode, Value) {
    approve_on(&s.state, s.company, token, id, login).await
}

async fn approve_on(
    state: &AppState,
    company: Uuid,
    token: &str,
    id: &str,
    login: &str,
) -> (StatusCode, Value) {
    call(
        state,
        token,
        "POST",
        &format!("/companies/{company}/vat-filings/{id}/approve"),
        json!({ "code": "one-time-code", "state": login }),
    )
    .await
}

async fn read(s: &Setup, id: &str) -> Value {
    let (status, body) = call(
        &s.state,
        &s.token,
        "GET",
        &format!("/companies/{}/vat-filings/{id}", s.company),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// The job runs in the background: wait until it has settled. A deadline
/// in wall time, not a count of polls: on a loaded CI runner a run that
/// takes a second locally took over ten.
async fn settled(s: &Setup, id: &str) -> Value {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        // The job first: a filing read after it closed has the final flags.
        let busy = regnmed_db::vat_filing::job_open(&s.state.pool, s.company, id.parse().unwrap())
            .await
            .unwrap();
        let filing = read(s, id).await;
        let moving = ["godkjent", "oppretter_instans", "instans_opprettet"]
            .contains(&filing["status"].as_str().unwrap());
        if !moving && !busy {
            return filing;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let job: Vec<(String, i32)> =
        sqlx::query_as("select status, attempts from job where company_id = $1 and dedup_key = $2")
            .bind(s.company)
            .bind(regnmed_db::vat_filing::dedup_key(id.parse().unwrap()))
            .fetch_all(&s.state.pool)
            .await
            .unwrap();
    panic!(
        "the filing never settled in {:?} (job {job:?}): {}",
        started.elapsed(),
        read(s, id).await
    );
}

async fn approved_and_settled(s: &Setup) -> (String, Value) {
    let (id, login) = start(s).await;
    let (status, body) = approve(s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let filing = settled(s, &id).await;
    (id, filing)
}

async fn again(s: &Setup, id: &str) -> Value {
    let login = sign_in_again(s, &s.token, id).await;
    let (status, body) = approve(s, &s.token, id, &login).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    settled(s, id).await
}

async fn sign_in_again(s: &Setup, token: &str, id: &str) -> String {
    let (status, body) = call(
        &s.state,
        token,
        "POST",
        &format!("/companies/{}/vat-filings/{id}/authorize", s.company),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    sign_in(s, body["authorize_url"].as_str().unwrap())
}

async fn event_kinds(s: &Setup, id: &str) -> Vec<String> {
    regnmed_db::vat_filing::events(&s.state.pool, id.parse().unwrap())
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect()
}

async fn job_closed(s: &Setup, id: &str) {
    for _ in 0..200 {
        if !regnmed_db::vat_filing::job_open(&s.state.pool, s.company, id.parse().unwrap())
            .await
            .unwrap()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the job never finished");
}

/// The revisjon report's account of the filings.
async fn audit_check(s: &Setup) -> Value {
    let (status, report) = call(
        &s.state,
        &s.token,
        "GET",
        &format!("/companies/{}/reports/revisjon", s.company),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    report["kontroller"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["navn"] == "Mva-meldinger")
        .cloned()
        .expect("the Mva-meldinger check")
}

async fn grants(s: &Setup, id: &str) -> i64 {
    sqlx::query_scalar("select count(*) from vat_filing_grant where filing_id = $1")
        .bind(id.parse::<Uuid>().unwrap())
        .fetch_one(&s.state.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn an_approved_melding_is_filed_and_the_answer_arrives_without_anyone() {
    let Some(s) = setup().await else { return };
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "sendt", "{filing}");
    assert_eq!(filing["godkjent_av"], "Kari Nordmann");
    assert_eq!(filing["betaling"]["beloep"], "2500");
    assert_eq!(s.mock.lock().unwrap().creates, 1);
    assert_eq!(s.mock.lock().unwrap().melding_uploads, 1);
    assert_eq!(s.mock.lock().unwrap().nexts, 2);

    // Neither token nor national ID is readable anywhere we stored them.
    let stored: String = sqlx::query_scalar(
        "select coalesce((select string_agg(e::text, ' ') from vat_filing_event e
                          where filing_id = $1), '')
             || coalesce((select string_agg(encode(sealed, 'escape'), ' ')
                          from vat_filing_grant where filing_id = $1), '')",
    )
    .bind(id.parse::<Uuid>().unwrap())
    .fetch_one(&s.state.pool)
    .await
    .unwrap();
    for secret in [PID, ID_TOKEN, ALTINN_TOKEN] {
        assert!(!stored.contains(secret), "{secret} is stored in clear");
    }
    assert!(stored.contains("pairwise-sub"), "the approver is recorded");

    s.mock.lock().unwrap().feedback = true;
    regnmed_db::vat_filing::reconcile(&s.state.pool, &s.worker)
        .await
        .unwrap();
    let done = read(&s, &id).await;
    assert_eq!(done["status"], "kvittert", "{done}");
    assert_eq!(grants(&s, &id).await, 0, "the token is gone with the work");

    // The revisor sees the filing and can compare the receipt byte for byte.
    let check = audit_check(&s).await;
    assert_eq!(check["ok"], true, "{check}");
    let detail = check["detalj"].as_str().unwrap();
    let receipt = hex::encode(Sha256::digest(b"%PDF-receipt"));
    assert!(
        detail.contains("2026 termin 1 kvittert, godkjent av Kari Nordmann"),
        "{detail}"
    );
    assert!(
        detail.contains(&format!("kvittering sha256 {receipt}")),
        "{detail}"
    );
    let (status, _) = call(
        &s.state,
        &s.token,
        "POST",
        &format!("/companies/{}/vat-filings", s.company),
        json!({ "year": 2026, "period": 1 }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a filed period stays filed"
    );
}

#[tokio::test]
async fn only_the_person_who_started_can_finish_and_only_once() {
    let Some(s) = setup().await else { return };
    let (id, login) = start(&s).await;

    let colleague = member(&s.state, &s.idp, s.company, "admin").await;
    let (status, _) = approve(&s, &colleague, &id, &login).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "someone else's sign-in");
    assert_eq!(read(&s, &id).await["status"], "opprettet");

    let (status, body) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let filing = settled(&s, &id).await;
    assert_eq!(filing["status"], "sendt");

    let (status, _) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a replayed sign-in");
    assert_eq!(s.mock.lock().unwrap().creates, 1);
}

#[tokio::test]
async fn a_forged_id_token_approves_nothing() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().wrong_nonce = true;
    let (id, login) = start(&s).await;
    let (status, _) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "wrong nonce");

    {
        let mut m = s.mock.lock().unwrap();
        m.wrong_nonce = false;
        m.signing_key = Some(TestIdp::new().encoding_key);
    }
    let (id2, login) = start(&s).await;
    assert_eq!(id, id2, "still the same open filing");
    let (status, _) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "wrong signature");
    assert_eq!(read(&s, &id).await["status"], "opprettet");
    assert_eq!(grants(&s, &id).await, 0);
    assert_eq!(s.mock.lock().unwrap().creates, 0);
}

#[tokio::test]
async fn a_ledger_that_changed_after_the_click_is_not_filed() {
    let Some(s) = setup().await else { return };
    let (id, login) = start(&s).await;
    regnmed_db::post_voucher(&s.state.pool, s.company, &sale(4_000_00), "test")
        .await
        .unwrap();
    let (status, body) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(s.mock.lock().unwrap().creates, 0);

    let (next, _) = start(&s).await;
    assert_ne!(next, id, "the changed melding is a new filing");
    assert_eq!(read(&s, &id).await["status"], "avbrutt");
}

#[tokio::test]
async fn validation_findings_stop_the_filing() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().validation = Some(REFUSED);
    let (_, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "avvik", "{filing}");
    assert_eq!(filing["avvik"][0], "Summen stemmer ikke");
    assert_eq!(s.mock.lock().unwrap().creates, 0, "nothing reached Altinn");
}

#[tokio::test]
async fn an_uncertain_step_resumes_the_same_instance() {
    let Some(s) = setup().await else { return };
    // The confirmation took effect, but the answer was lost.
    s.mock.lock().unwrap().next_fails_once = Some((2, 502, true));
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "uavklart", "{filing}");
    assert!(filing["kan_godkjennes"].as_bool().unwrap());
    assert!(
        !filing["kan_avbrytes"].as_bool().unwrap(),
        "it may be filed"
    );

    let resumed = again(&s, &id).await;
    assert_eq!(resumed["status"], "sendt", "{resumed}");
    let m = s.mock.lock().unwrap();
    assert_eq!(m.creates, 1, "never a second instance");
    assert_eq!(m.nexts, 2, "the step that happened is not repeated");
    assert_eq!(m.melding_uploads, 1);
}

#[tokio::test]
async fn a_changed_ledger_stops_an_unsent_instance_but_not_a_sent_one() {
    let Some(s) = setup().await else { return };
    // Refused at confirmation: an instance exists, but nothing was filed.
    s.mock.lock().unwrap().next_fails_once = Some((2, 403, false));
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "feilet", "{filing}");
    assert!(!filing["kan_avbrytes"].as_bool().unwrap());
    regnmed_db::post_voucher(&s.state.pool, s.company, &sale(4_000_00), "test")
        .await
        .unwrap();

    let stopped = again(&s, &id).await;
    assert_eq!(stopped["status"], "avbrutt", "{stopped}");
    assert_eq!(s.mock.lock().unwrap().task, 1, "still not confirmed");

    let (next, _) = start(&s).await;
    assert_ne!(next, id);
}

#[tokio::test]
async fn an_uncertain_create_is_never_sent_again() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().create_fails = Some(502);
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "uavklart", "{filing}");
    assert!(!filing["kan_godkjennes"].as_bool().unwrap());
    assert!(!filing["kan_avbrytes"].as_bool().unwrap());
    assert!(filing["kan_lukkes"].as_bool().unwrap());

    let (status, _) = call(
        &s.state,
        &s.token,
        "POST",
        &format!("/companies/{}/vat-filings/{id}/authorize", s.company),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(s.mock.lock().unwrap().creates, 1);
    let (status, _) = call(
        &s.state,
        &s.token,
        "POST",
        &format!("/companies/{}/vat-filings/{id}/abandon", s.company),
        json!({}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "only a person's check closes it"
    );
}

#[tokio::test]
async fn a_person_without_the_altinn_right_is_told_what_to_get() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().create_fails = Some(403);
    let (_, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "feilet", "{filing}");
    assert_eq!(filing["mangler_altinn_rett"], true);
    assert!(
        filing["melding"]
            .as_str()
            .unwrap()
            .contains("klientadministrator")
    );
    assert!(filing["kan_godkjennes"].as_bool().unwrap());
}

#[tokio::test]
async fn an_expired_token_asks_for_a_new_approval() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().expires_in = Some(-1);
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "ma_godkjennes", "{filing}");
    assert_eq!(s.mock.lock().unwrap().creates, 0);
    s.mock.lock().unwrap().expires_in = None;
    assert_eq!(again(&s, &id).await["status"], "sendt");
}

#[tokio::test]
async fn an_accountant_can_file_but_an_auditor_cannot() {
    let Some(s) = setup().await else { return };
    let auditor = member(&s.state, &s.idp, s.company, "revisor").await;
    let (status, body) = call(
        &s.state,
        &auditor,
        "POST",
        &format!("/companies/{}/vat-filings", s.company),
        json!({ "year": 2026, "period": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let bookkeeper = member(&s.state, &s.idp, s.company, "bokforing").await;
    let (status, body) = call(
        &s.state,
        &bookkeeper,
        "POST",
        &format!("/companies/{}/vat-filings", s.company),
        json!({ "year": 2026, "period": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// Finding 1: the filing can be replaced while the person is at ID-porten.
#[tokio::test]
async fn a_filing_replaced_during_sign_in_is_never_approved() {
    let Some(s) = setup().await else { return };
    let (id, login) = start(&s).await;
    let (reached, release) = gate(&s, "token");
    let (state, company, token) = (s.state.clone(), s.company, s.token.clone());
    let (first, first_login) = (id.clone(), login.clone());
    let pending =
        tokio::spawn(
            async move { approve_on(&state, company, &token, &first, &first_login).await },
        );
    reached.notified().await;

    regnmed_db::post_voucher(&s.state.pool, s.company, &sale(4_000_00), "test")
        .await
        .unwrap();
    let (next, _) = start(&s).await;
    assert_ne!(next, id);
    release.notify_one();

    let (status, body) = pending.await.unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(read(&s, &id).await["status"], "avbrutt");
    assert!(!event_kinds(&s, &id).await.contains(&"godkjent".to_string()));
    assert_eq!(s.mock.lock().unwrap().creates, 0);
}

/// Finding 1: two approvals racing make one approval and one job.
#[tokio::test]
async fn concurrent_approvals_queue_exactly_one_job() {
    use regnmed_db::vat_filing::{ApprovalOutcome, Approver};
    let Some(s) = setup().await else { return };
    let (id, _) = start(&s).await;
    let filing_id: Uuid = id.parse().unwrap();
    let filing = regnmed_db::vat_filing::get(&s.state.pool, s.company, filing_id)
        .await
        .unwrap()
        .unwrap();
    let person: Uuid =
        sqlx::query_scalar("select person_id from company_member where company_id = $1 limit 1")
            .bind(s.company)
            .fetch_one(&s.state.pool)
            .await
            .unwrap();
    let approver = || Approver {
        person_id: person,
        name: "Kari Nordmann".into(),
        sub: "pairwise-sub".into(),
        acr: "idporten-loa-high".into(),
        auth_time: chrono::Utc::now(),
        envelope_xml: "<e/>".into(),
    };
    let expires = chrono::Utc::now() + chrono::Duration::seconds(60);
    let (a1, a2) = (approver(), approver());
    let (first, second) = tokio::join!(
        regnmed_db::vat_filing::approve(
            &s.state.pool,
            &filing,
            "iss",
            "client",
            &a1,
            b"sealed",
            expires,
        ),
        regnmed_db::vat_filing::approve(
            &s.state.pool,
            &filing,
            "iss",
            "client",
            &a2,
            b"sealed",
            expires,
        ),
    );
    let outcomes = [first.unwrap(), second.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| matches!(o, ApprovalOutcome::Approved { .. }))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| matches!(o, ApprovalOutcome::NotApprovable))
            .count(),
        1
    );
    let kinds = event_kinds(&s, &id).await;
    assert_eq!(kinds.iter().filter(|k| *k == "godkjent").count(), 1);
    // Nobody works this one: keep it out of other tests' sweeps.
    sqlx::query("update job set status = 'failed', finished_at = now() where dedup_key = $1")
        .bind(regnmed_db::vat_filing::dedup_key(filing_id))
        .execute(&s.state.pool)
        .await
        .unwrap();
}

/// Finding 2.
#[tokio::test]
async fn an_undocumented_task_after_confirmation_is_not_called_filed() {
    for odd in [AfterConfirm::UnknownTask, AfterConfirm::NoProcess] {
        let Some(s) = setup().await else { return };
        s.mock.lock().unwrap().after_confirm = odd;
        let (id, filing) = approved_and_settled(&s).await;
        assert_eq!(filing["status"], "uavklart", "{filing}");
        assert!(!event_kinds(&s, &id).await.contains(&"sendt".to_string()));
    }
}

#[tokio::test]
async fn an_ended_process_counts_as_filed() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().after_confirm = AfterConfirm::Ended;
    let (_, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "sendt", "{filing}");
}

/// Finding 3.
#[tokio::test]
async fn a_second_approver_never_confirms_the_first_persons_envelope() {
    for delete_fails in [false, true] {
        let Some(s) = setup().await else { return };
        {
            let mut m = s.mock.lock().unwrap();
            m.next_fails_once = Some((2, 403, false));
            m.delete_fails = delete_fails;
        }
        let (id, filing) = approved_and_settled(&s).await;
        assert_eq!(filing["status"], "feilet", "{filing}");
        assert_eq!(s.mock.lock().unwrap().task, 1, "waiting at confirmation");

        let colleague = member(&s.state, &s.idp, s.company, "admin").await;
        let login = sign_in_again(&s, &colleague, &id).await;
        let (status, body) = approve(&s, &colleague, &id, &login).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let aborted = settled(&s, &id).await;
        assert_eq!(aborted["status"], "avbrutt", "{aborted}");
        assert!(
            aborted["melding"]
                .as_str()
                .unwrap()
                .contains("annen person")
        );
        {
            let m = s.mock.lock().unwrap();
            assert_eq!(m.nexts, 2, "the colleague did not confirm it");
            assert_eq!(m.task, 1);
            assert_eq!(m.deletes, 1);
        }

        // A failed delete blocks nothing: a fresh filing gets a new instance.
        let (status, body) = call(
            &s.state,
            &colleague,
            "POST",
            &format!("/companies/{}/vat-filings", s.company),
            json!({ "year": 2026, "period": 1 }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let next = body["filing"]["id"].as_str().unwrap().to_string();
        assert_ne!(next, id);
        let login = sign_in(&s, body["authorize_url"].as_str().unwrap());
        let (status, body) = approve(&s, &colleague, &next, &login).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(settled(&s, &next).await["status"], "sendt");
        assert_eq!(s.mock.lock().unwrap().creates, 2);
    }
}

/// Finding 4.
#[tokio::test]
async fn an_old_sign_in_approves_nothing() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().auth_time_offset = -3600;
    let (id, login) = start(&s).await;
    let (status, _) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(read(&s, &id).await["status"], "opprettet");
}

#[tokio::test]
async fn a_person_is_bound_to_one_id_porten_identity() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().create_fails = Some(403);
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "feilet", "{filing}");

    {
        let mut m = s.mock.lock().unwrap();
        m.create_fails = None;
        m.sub = Some("someone-else");
    }
    let login = sign_in_again(&s, &s.token, &id).await;
    let (status, body) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("annen person"), "{body}");
    assert_eq!(read(&s, &id).await["status"], "feilet");

    s.mock.lock().unwrap().sub = None;
    assert_eq!(again(&s, &id).await["status"], "sendt");
    let stored: Vec<String> =
        sqlx::query_scalar("select sub from idporten_identity where sub = 'someone-else'")
            .fetch_all(&s.state.pool)
            .await
            .unwrap();
    assert!(stored.is_empty(), "a refused sign-in binds nothing");
}

/// Finding 5.
#[tokio::test]
async fn a_failed_token_exchange_claims_nothing() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().exchange_fails = Some(502);
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "ma_godkjennes", "{filing}");
    assert!(!filing["melding"].as_str().unwrap().contains("Ingenting"));
    assert!(!event_kinds(&s, &id).await.contains(&"feilet".to_string()));
    assert_eq!(grants(&s, &id).await, 0);
    s.mock.lock().unwrap().exchange_fails = None;
    assert_eq!(again(&s, &id).await["status"], "sendt");
}

#[tokio::test]
async fn a_failed_exchange_keeps_an_unaccounted_create_closed() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().exchange_fails = Some(502);
    let (id, _) = approved_and_settled(&s).await;
    // A worker died right after it sent the create.
    sqlx::query(
        "insert into vat_filing_event (id, filing_id, kind, created_at)
         values ($1, $2, 'oppretter_instans', clock_timestamp())",
    )
    .bind(Uuid::now_v7())
    .bind(id.parse::<Uuid>().unwrap())
    .execute(&s.state.pool)
    .await
    .unwrap();
    let filing = again(&s, &id).await;
    assert_eq!(filing["status"], "uavklart", "{filing}");
    assert!(!filing["kan_godkjennes"].as_bool().unwrap());
    s.mock.lock().unwrap().exchange_fails = None;
    let (status, _) = call(
        &s.state,
        &s.token,
        "POST",
        &format!("/companies/{}/vat-filings/{id}/authorize", s.company),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(s.mock.lock().unwrap().creates, 0, "no second create");
}

/// Finding 6, on the server's side: a dead job offers the way on.
#[tokio::test]
async fn a_job_that_died_is_not_shown_as_running() {
    let Some(mut s) = setup().await else { return };
    let current = s.state.idporten.clone().unwrap();
    let base = s.mock.lock().unwrap().base.clone();
    s.state.idporten = Some(Arc::new(IdportenRail {
        client: IdportenClient::new(
            base,
            "client",
            "client-secret",
            "https://test.regnmed.invalid/mva-godkjenning",
        )
        .unwrap(),
        key: current.key.clone(),
        local_worker: None,
    }));
    let (id, login) = start(&s).await;
    let (status, body) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    job_closed(&s, &id).await;
    let filing = read(&s, &id).await;
    assert_eq!(filing["status"], "godkjent");
    assert_eq!(filing["jobb_aktiv"], false);
    assert_eq!(filing["kan_godkjennes"], true);
}

/// Finding 7 and R3: another run has the filing, so this job waits and
/// stays open rather than finishing while that run is alive.
#[tokio::test]
async fn a_job_waits_and_stays_open_while_another_run_holds_the_filing() {
    let Some(s) = setup().await else { return };
    let (id, login) = start(&s).await;
    let mut holder = s.state.pool.acquire().await.unwrap();
    holder.close_on_drop();
    let held: bool = sqlx::query_scalar("select pg_try_advisory_lock(hashtextextended($1, 0))")
        .bind(regnmed_db::vat_filing::run_lock_key(id.parse().unwrap()))
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    assert!(held);

    let (status, body) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job_id: Uuid = body["job_id"].as_str().unwrap().parse().unwrap();
    let mut queued = None;
    for _ in 0..200 {
        let job = job::get(&s.state.pool, s.company, job_id)
            .await
            .unwrap()
            .unwrap();
        if job.status == "queued" && job.started_at.is_some() {
            queued = Some(job);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let job = queued.expect("the job went back to the queue");
    assert_eq!(job.attempts, 0, "waiting uses up no attempt");
    assert!(read(&s, &id).await["jobb_aktiv"].as_bool().unwrap());
    assert_eq!(grants(&s, &id).await, 1);
    {
        let m = s.mock.lock().unwrap();
        assert_eq!((m.creates, m.nexts), (0, 0));
    }

    drop(holder);
    let runtime = job::Runtime {
        vat_filing: Some(s.worker.clone()),
        ..Default::default()
    };
    let mut claimed = None;
    for _ in 0..200 {
        claimed = job::claim(&s.state.pool, job_id, "test-worker")
            .await
            .unwrap();
        if claimed.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    job::process(&s.state.pool, &claimed.unwrap(), "test-worker", &runtime)
        .await
        .unwrap();
    assert_eq!(read(&s, &id).await["status"], "sendt");
    assert_eq!(s.mock.lock().unwrap().creates, 1);
}

/// R3: a run that lost its lock makes no further calls that change Altinn.
#[tokio::test]
async fn a_run_that_lost_its_lock_stops_before_the_next_altinn_call() {
    let Some(s) = setup().await else { return };
    let (reached, release) = gate(&s, "create");
    let (id, login) = start(&s).await;
    let (status, body) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job_id: Uuid = body["job_id"].as_str().unwrap().parse().unwrap();
    reached.notified().await;
    let killed: bool = sqlx::query_scalar(
        "select pg_terminate_backend(pid) from pg_locks
         where locktype = 'advisory' and granted and objsubid = 1
           and ((classid::bigint << 32) | objid::bigint) = hashtextextended($1, 0)",
    )
    .bind(regnmed_db::vat_filing::run_lock_key(id.parse().unwrap()))
    .fetch_one(&s.state.pool)
    .await
    .unwrap();
    assert!(killed);
    release.notify_one();
    for _ in 0..200 {
        let job = job::get(&s.state.pool, s.company, job_id)
            .await
            .unwrap()
            .unwrap();
        if job.status == "queued" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let job = job::get(&s.state.pool, s.company, job_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.status, "queued", "it comes back instead of finishing");
    let m = s.mock.lock().unwrap();
    assert_eq!(
        (m.melding_uploads, m.nexts),
        (0, 0),
        "no call after the lock was lost"
    );
}

#[tokio::test]
async fn a_late_run_cannot_overwrite_a_newer_state() {
    let Some(s) = setup().await else { return };
    let (reached, release) = gate(&s, "create");
    let (id, login) = start(&s).await;
    let (status, body) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    reached.notified().await;
    // Something else closed the filing while this run waited on Altinn.
    sqlx::query(
        "insert into vat_filing_event (id, filing_id, kind, created_at)
         values ($1, $2, 'avbrutt', clock_timestamp())",
    )
    .bind(Uuid::now_v7())
    .bind(id.parse::<Uuid>().unwrap())
    .execute(&s.state.pool)
    .await
    .unwrap();
    release.notify_one();
    job_closed(&s, &id).await;
    assert_eq!(read(&s, &id).await["status"], "avbrutt");
    assert!(
        !event_kinds(&s, &id)
            .await
            .contains(&"instans_opprettet".to_string())
    );
    let m = s.mock.lock().unwrap();
    assert_eq!((m.melding_uploads, m.nexts), (0, 0));
}

/// Finding 8.
#[tokio::test]
async fn a_ledger_change_during_validation_stops_before_the_create() {
    let Some(s) = setup().await else { return };
    let (reached, release) = gate(&s, "valider");
    let (id, login) = start(&s).await;
    let (status, body) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    reached.notified().await;
    regnmed_db::post_voucher(&s.state.pool, s.company, &sale(4_000_00), "test")
        .await
        .unwrap();
    release.notify_one();
    let filing = settled(&s, &id).await;
    assert_eq!(filing["status"], "avbrutt", "{filing}");
    assert_eq!(s.mock.lock().unwrap().creates, 0);
}

#[tokio::test]
async fn a_ledger_change_before_confirmation_stops_the_confirm() {
    let Some(s) = setup().await else { return };
    let (reached, release) = gate(&s, "next");
    let (id, login) = start(&s).await;
    let (status, body) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    reached.notified().await;
    regnmed_db::post_voucher(&s.state.pool, s.company, &sale(4_000_00), "test")
        .await
        .unwrap();
    release.notify_one();
    let filing = settled(&s, &id).await;
    assert_eq!(filing["status"], "avbrutt", "{filing}");
    let m = s.mock.lock().unwrap();
    assert_eq!(m.nexts, 1, "the confirming step was never taken");
    assert_eq!(m.task, 1);
}

/// The open question: only an error verdict makes a missing receipt final.
#[tokio::test]
async fn a_result_without_a_receipt_waits_unless_it_is_an_error() {
    let Some(s) = setup().await else { return };
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "sendt");
    {
        let mut m = s.mock.lock().unwrap();
        m.feedback = true;
        m.no_receipt = true;
        m.result = Some(CLEAN);
    }
    regnmed_db::vat_filing::reconcile(&s.state.pool, &s.worker)
        .await
        .unwrap();
    assert_eq!(
        read(&s, &id).await["status"],
        "sendt",
        "a clean result waits"
    );
    assert_eq!(grants(&s, &id).await, 1);

    // The stored file is the first one received, so a later error needs a new filing.
    let Some(r) = setup().await else { return };
    let (rejected, filing) = approved_and_settled(&r).await;
    assert_eq!(filing["status"], "sendt");
    {
        let mut m = r.mock.lock().unwrap();
        m.feedback = true;
        m.no_receipt = true;
        m.result = Some(REFUSED);
    }
    regnmed_db::vat_filing::reconcile(&r.state.pool, &r.worker)
        .await
        .unwrap();
    let done = read(&r, &rejected).await;
    assert_eq!(done["status"], "avvist", "{done}");
    assert_eq!(done["avvik"][0], "Summen stemmer ikke");
}

/// R1: with an instance on record, an unusable token claims nothing.
#[tokio::test]
async fn a_lost_token_after_the_instance_exists_is_unresolved() {
    for expired in [false, true] {
        let Some(s) = setup().await else { return };
        s.mock.lock().unwrap().next_fails_once = Some((2, 403, false));
        let (id, filing) = approved_and_settled(&s).await;
        assert_eq!(filing["status"], "feilet", "{filing}");
        assert!(filing["kan_vaere_sendt"].as_bool().unwrap());
        assert!(!filing["melding"].as_str().unwrap().contains("Ingenting"));
        {
            let mut m = s.mock.lock().unwrap();
            if expired {
                m.expires_in = Some(-1);
            } else {
                m.exchange_fails = Some(502);
            }
        }
        let filing = again(&s, &id).await;
        assert_eq!(filing["status"], "uavklart", "{filing}");
        assert!(filing["melding"].as_str().unwrap().contains("Sjekk Altinn"));
        assert!(!filing["melding"].as_str().unwrap().contains("Ingenting"));
        assert!(
            !event_kinds(&s, &id)
                .await
                .contains(&"ma_godkjennes".to_string())
        );
        assert_eq!(s.mock.lock().unwrap().nexts, 2);
    }
}

/// R2: after an uncertain step, only a confirmed delete closes the filing.
#[tokio::test]
async fn an_uncertain_filing_closes_only_after_a_confirmed_delete() {
    for delete_fails in [true, false] {
        let Some(s) = setup().await else { return };
        {
            let mut m = s.mock.lock().unwrap();
            m.next_fails_once = Some((2, 502, false));
            m.delete_fails = delete_fails;
        }
        let (id, filing) = approved_and_settled(&s).await;
        assert_eq!(filing["status"], "uavklart", "{filing}");
        regnmed_db::post_voucher(&s.state.pool, s.company, &sale(4_000_00), "test")
            .await
            .unwrap();
        let after = again(&s, &id).await;
        assert_eq!(s.mock.lock().unwrap().deletes, 1);
        assert_eq!(s.mock.lock().unwrap().nexts, 2, "never confirmed");
        let want = if delete_fails { "uavklart" } else { "avbrutt" };
        assert_eq!(after["status"], want, "{after}");
    }
}

/// R4.
#[tokio::test]
async fn a_person_closes_an_unresolved_filing_with_a_note() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().create_fails = Some(502);
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "uavklart");
    let check = audit_check(&s).await;
    assert_eq!(check["ok"], false, "an unknown fate is an avvik: {check}");
    let uri = format!("/companies/{}/vat-filings/{id}/close-unresolved", s.company);

    for body in [
        json!({ "funn": "ikke_levert", "notat": "  ", "sjekket_altinn": true }),
        json!({ "funn": "ikke_levert", "notat": "Ingen innsending", "sjekket_altinn": false }),
        json!({ "funn": "kanskje", "notat": "Ingen innsending", "sjekket_altinn": true }),
        json!({ "funn": "", "notat": "Ingen innsending", "sjekket_altinn": true }),
    ] {
        let (status, _) = call(&s.state, &s.token, "POST", &uri, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let auditor = member(&s.state, &s.idp, s.company, "revisor").await;
    let body = json!({
        "funn": "ikke_levert",
        "notat": "Ingen innsending i Altinn",
        "sjekket_altinn": true,
    });
    let (status, _) = call(&s.state, &auditor, "POST", &uri, body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, closed) = call(&s.state, &s.token, "POST", &uri, body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(closed["status"], "lukket_etter_kontroll");
    assert_eq!(audit_check(&s).await["ok"], true, "closed with a finding");
    let events = regnmed_db::vat_filing::events(&s.state.pool, id.parse().unwrap())
        .await
        .unwrap();
    let last = events.last().unwrap();
    assert_eq!(last.detail["notat"], "Ingen innsending i Altinn");
    assert_eq!(last.detail["funn"], "ikke_levert");
    assert_eq!(closed["lukking"]["funn"], "ikke_levert");
    assert_eq!(closed["lukking"]["notat"], "Ingen innsending i Altinn");
    assert_eq!(closed["lukking"]["av"], "Kari Nordmann");
    assert!(closed["lukking"]["tidspunkt"].is_string());
    assert_eq!(last.detail["name"], "Kari Nordmann");
    assert!(last.detail["person_id"].is_string());
    assert!(
        !events.iter().any(|e| e.kind == "sendt"),
        "never marked as filed"
    );

    let (status, _) = call(&s.state, &s.token, "POST", &uri, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "closed once");
    s.mock.lock().unwrap().create_fails = None;
    let (next, login) = start(&s).await;
    assert_ne!(next, id, "a new filing can start");
    let (status, body) = approve(&s, &s.token, &next, &login).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(settled(&s, &next).await["status"], "sendt");
}

/// R5.
#[tokio::test]
async fn an_unreadable_validation_answer_files_nothing() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().validation = Some("<html>200 OK</html>");
    let (_, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "feilet", "{filing}");
    assert_eq!(s.mock.lock().unwrap().creates, 0);
}

/// R6.
#[tokio::test]
async fn an_operator_rebind_is_recorded_and_lets_the_new_identity_in() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().create_fails = Some(403);
    let (id, _) = approved_and_settled(&s).await;
    let person: Uuid =
        sqlx::query_scalar("select person_id from company_member where company_id = $1 limit 1")
            .bind(s.company)
            .fetch_one(&s.state.pool)
            .await
            .unwrap();
    let base = s.mock.lock().unwrap().base.clone();
    {
        let mut m = s.mock.lock().unwrap();
        m.create_fails = None;
        m.sub = Some("new-pairwise-sub");
    }
    let login = sign_in_again(&s, &s.token, &id).await;
    let (status, _) = approve(&s, &s.token, &id, &login).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "still the old identity");

    let rebind = |reason: &'static str| {
        let (pool, base) = (s.state.pool.clone(), base.clone());
        async move {
            regnmed_db::vat_filing::rebind_identity(
                &pool,
                person,
                &base,
                "client",
                "Ola Operatør",
                reason,
            )
            .await
        }
    };
    let refused = rebind("  ").await.unwrap_err().to_string();
    assert!(refused.contains("needs a reason"), "{refused}");
    rebind("Sak 123: byttet eID").await.unwrap();
    assert_eq!(again(&s, &id).await["status"], "sendt");

    let rows: Vec<(Option<String>, String, Option<String>, String)> = sqlx::query_as(
        "select sub, source, reason, recorded_by from idporten_identity
         where person_id = $1 order by created_at, id",
    )
    .bind(person)
    .fetch_all(&s.state.pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 3, "history is kept: {rows:?}");
    assert_eq!(rows[0].0.as_deref(), Some("pairwise-sub"));
    assert_eq!(rows[1].0, None);
    assert_eq!(rows[1].1, "operator");
    assert_eq!(rows[1].2.as_deref(), Some("Sak 123: byttet eID"));
    assert_eq!(rows[1].3, "Ola Operatør");
    assert_eq!(rows[2].0.as_deref(), Some("new-pairwise-sub"));
    assert!(!format!("{rows:?}").contains(PID));
}

/// R7.
#[tokio::test]
async fn one_bad_grant_does_not_stop_reconciliation() {
    let Some(s) = setup().await else { return };
    // An older filing whose stored token can no longer be opened.
    let Some(other) = setup().await else { return };
    let (bad, filing) = approved_and_settled(&other).await;
    assert_eq!(filing["status"], "sendt");
    sqlx::query("update vat_filing_grant set sealed = $2 where filing_id = $1")
        .bind(bad.parse::<Uuid>().unwrap())
        .bind(vec![0u8; 64])
        .execute(&s.state.pool)
        .await
        .unwrap();

    let (good, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "sendt");
    s.mock.lock().unwrap().feedback = true;
    regnmed_db::vat_filing::reconcile(&s.state.pool, &s.worker)
        .await
        .unwrap();
    assert_eq!(read(&s, &good).await["status"], "kvittert");
    assert_eq!(read(&other, &bad).await["status"], "sendt");
}

/// Blocker 1: a person's finding of "delivered" keeps the term closed, and is
/// not presented as a receipt regnmed fetched.
#[tokio::test]
async fn a_term_found_delivered_stays_closed() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().create_fails = Some(502);
    let (id, _) = approved_and_settled(&s).await;
    let (status, closed) = call(
        &s.state,
        &s.token,
        "POST",
        &format!("/companies/{}/vat-filings/{id}/close-unresolved", s.company),
        json!({ "funn": "levert", "notat": "Kvittering i Altinn", "sjekket_altinn": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{closed}");
    assert_eq!(closed["status"], "lukket_etter_kontroll");
    assert_eq!(closed["lukking"]["funn"], "levert");
    assert_eq!(closed["lukking"]["notat"], "Kvittering i Altinn");
    assert_eq!(closed["kan_godkjennes"], false);
    assert_eq!(closed["dokumenter"], json!([]), "regnmed holds no receipt");
    let kinds = event_kinds(&s, &id).await;
    assert!(!kinds.contains(&"sendt".to_string()));
    assert!(!kinds.contains(&"kvittert".to_string()));

    s.mock.lock().unwrap().create_fails = None;
    let (status, body) = call(
        &s.state,
        &s.token,
        "POST",
        &format!("/companies/{}/vat-filings", s.company),
        json!({ "year": 2026, "period": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string().contains("levert ifølge kontrollen"),
        "{body}"
    );
    assert_eq!(s.mock.lock().unwrap().creates, 1);
}

/// The return names who signed in with ID-porten, not the regnmed account.
#[tokio::test]
async fn the_envelope_and_the_portal_name_the_person_who_signed_in() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().signer = Some("VERD DIODE");
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "sendt", "{filing}");
    assert_eq!(filing["godkjent_av"], "VERD DIODE");
    let envelope = s.mock.lock().unwrap().envelope.clone().unwrap();
    assert_eq!(
        envelope
            .matches("<opprettetAv>VERD DIODE</opprettetAv>")
            .count(),
        2
    );
    assert!(!envelope.contains("Kari Nordmann"), "{envelope}");

    let stored: String = sqlx::query_scalar(
        "select string_agg(detail::text, ' ') from vat_filing_event where filing_id = $1",
    )
    .bind(id.parse::<Uuid>().unwrap())
    .fetch_one(&s.state.pool)
    .await
    .unwrap();
    assert!(!stored.contains(PID), "the profile's national ID is stored");
}

/// A refusal in validation names the same person, and a profile that cannot
/// be read stops the filing before anything exists in Altinn.
#[tokio::test]
async fn the_signer_is_known_before_anything_is_created() {
    let Some(s) = setup().await else { return };
    {
        let mut m = s.mock.lock().unwrap();
        m.signer = Some("VERD DIODE");
        m.validation = Some(REFUSED);
    }
    let (_, refused) = approved_and_settled(&s).await;
    assert_eq!(refused["status"], "avvik", "{refused}");
    assert_eq!(refused["godkjent_av"], "VERD DIODE");

    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().profile_fails = true;
    let (_, failed) = approved_and_settled(&s).await;
    assert_eq!(failed["status"], "feilet", "{failed}");
    assert_eq!(s.mock.lock().unwrap().creates, 0);
    assert!(s.mock.lock().unwrap().envelope.is_none());
}

/// An account rebound to another ID-porten person keeps the same regnmed id,
/// so only Altinn's answer shows the envelope names someone else.
#[tokio::test]
async fn another_signer_never_confirms_the_first_persons_envelope() {
    let Some(s) = setup().await else { return };
    {
        let mut m = s.mock.lock().unwrap();
        m.signer = Some("VERD DIODE");
        m.next_fails_once = Some((2, 403, false));
    }
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "feilet", "{filing}");
    assert_eq!(s.mock.lock().unwrap().task, 1, "waiting at confirmation");

    s.mock.lock().unwrap().signer = Some("Bo Leser");
    let aborted = again(&s, &id).await;
    assert_eq!(aborted["status"], "avbrutt", "{aborted}");
    assert!(
        aborted["melding"]
            .as_str()
            .unwrap()
            .contains("annen person")
    );
    let m = s.mock.lock().unwrap();
    assert_eq!(m.nexts, 2, "the second person did not confirm it");
    assert_eq!(m.deletes, 1);
}

/// Blocker 2: a colleague who later only reads the instance did not file it.
#[tokio::test]
async fn the_person_who_confirmed_stays_the_approver_after_a_later_sign_in() {
    let Some(s) = setup().await else { return };
    // The confirm landed, but its answer was lost.
    s.mock.lock().unwrap().next_fails_once = Some((2, 502, true));
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "uavklart", "{filing}");
    assert_eq!(filing["godkjent_av"], "Kari Nordmann");

    s.mock.lock().unwrap().signer = Some("Bo Leser");
    let colleague = named_member(&s.state, &s.idp, s.company, "admin", "Bo Leser").await;
    let login = sign_in_again(&s, &colleague, &id).await;
    let (status, body) = approve(&s, &colleague, &id, &login).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let filed = settled(&s, &id).await;
    assert_eq!(filed["status"], "sendt", "{filed}");
    assert_eq!(filed["godkjent_av"], "Kari Nordmann", "{filed}");
    assert_eq!(s.mock.lock().unwrap().nexts, 2);
}

/// Blocker 3: a confirm with no clear answer is never sent again.
#[tokio::test]
async fn a_confirm_with_no_clear_answer_is_never_retried() {
    for delete_fails in [false, true] {
        let Some(s) = setup().await else { return };
        {
            let mut m = s.mock.lock().unwrap();
            m.next_fails_once = Some((2, 502, false));
            m.delete_fails = delete_fails;
        }
        let (id, filing) = approved_and_settled(&s).await;
        assert_eq!(filing["status"], "uavklart", "{filing}");
        assert_eq!(s.mock.lock().unwrap().task, 1, "reads as unconfirmed");

        let after = again(&s, &id).await;
        let want = if delete_fails { "uavklart" } else { "avbrutt" };
        assert_eq!(after["status"], want, "{after}");
        let m = s.mock.lock().unwrap();
        assert_eq!(m.nexts, 2, "the confirm is not sent a second time");
        assert_eq!(m.deletes, 1);
    }
}

/// Blocker 3: a later read's refusal says nothing about the earlier confirm.
#[tokio::test]
async fn a_later_read_refusal_does_not_settle_a_confirm() {
    let Some(s) = setup().await else { return };
    {
        let mut m = s.mock.lock().unwrap();
        m.next_fails_once = Some((2, 502, false));
        m.delete_fails = true;
    }
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "uavklart", "{filing}");

    s.mock.lock().unwrap().read_fails_once = Some(403);
    let refused = again(&s, &id).await;
    assert_eq!(refused["status"], "feilet", "{refused}");

    let after = again(&s, &id).await;
    assert_eq!(after["status"], "uavklart", "{after}");
    let m = s.mock.lock().unwrap();
    assert_eq!(m.nexts, 2, "the confirm is not sent a second time");
    assert_eq!(m.deletes, 1);
}

/// Blocker 3: a refused filling step is not a refusal of the earlier confirm.
#[tokio::test]
async fn a_refused_filling_step_does_not_settle_a_confirm() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().next_fails_once = Some((2, 502, false));
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "uavklart", "{filing}");
    {
        let mut m = s.mock.lock().unwrap();
        m.task = 0;
        m.next_fails_once = Some((3, 403, false));
        m.delete_fails = true;
    }
    let refused = again(&s, &id).await;
    assert_eq!(refused["status"], "feilet", "{refused}");

    s.mock.lock().unwrap().task = 1;
    let after = again(&s, &id).await;
    assert_eq!(after["status"], "uavklart", "{after}");
    assert_eq!(s.mock.lock().unwrap().nexts, 3);
}

/// Blocker 3: Altinn refusing that very confirm settles it, so it may be
/// tried again by the same person.
#[tokio::test]
async fn a_definitely_refused_confirm_may_be_tried_again() {
    let Some(s) = setup().await else { return };
    s.mock.lock().unwrap().next_fails_once = Some((2, 403, false));
    let (id, filing) = approved_and_settled(&s).await;
    assert_eq!(filing["status"], "feilet", "{filing}");
    assert!(
        event_kinds(&s, &id)
            .await
            .contains(&"bekreftelse_avvist".to_string())
    );
    let filed = again(&s, &id).await;
    assert_eq!(filed["status"], "sendt", "{filed}");
    let m = s.mock.lock().unwrap();
    assert_eq!(m.nexts, 3);
    assert_eq!(m.deletes, 0);
}
