//! Direct bank connection (docs/bank.md tier 2, #70): the consent flow and
//! the fetch, as transport around the existing reconciliation.
//!
//! Three things must hold, in this order: (1) without a configured provider
//! there are no buttons — the status says `konfigurert: false` and every
//! action refuses, never pretends; (2) a company without the add-on can
//! neither connect nor fetch; (3) the account a bank returns must be the one
//! the user said they were connecting, so a consent for another account can
//! never feed this ledger account. The bank credentials never reach us: the
//! user authenticates at the bank and we keep the provider's session
//! reference.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use chrono::Utc;
use regnmed_core::bankfeed::norwegian_bban;
use regnmed_db::bank_feed::{self, ADDON};
use regnmed_gov::enablebanking::{Client, FeedError};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

const COUNTRY: &str = "NO";
/// PSD2 allows up to 180 days between strong authentications; a bank may
/// allow less, and then its own maximum applies.
const MAX_CONSENT_DAYS: i64 = 180;

fn provider(state: &AppState) -> Result<&Client, ApiError> {
    state.bank_feed.as_deref().ok_or_else(|| {
        ApiError::BadRequest("bankkobling er ikke satt opp på denne installasjonen".into())
    })
}

async fn require_addon(state: &AppState, company_id: Uuid) -> Result<(), ApiError> {
    if bank_feed::addon_active(&state.pool, company_id, ADDON).await? {
        Ok(())
    } else {
        Err(ApiError::BadRequest(
            "selskapet har ikke tillegget Bankkobling".into(),
        ))
    }
}

/// A provider failure as a message a person can act on.
fn feed_error(e: FeedError) -> ApiError {
    ApiError::BadRequest(match e {
        FeedError::ConsentGone(_) => {
            "samtykket hos banken er utløpt eller trukket tilbake; koble til på nytt".into()
        }
        FeedError::RateLimited => {
            "banken tillater ikke flere hentinger akkurat nå; prøv igjen om noen timer".into()
        }
        FeedError::Other(e) => format!("banken svarte ikke som ventet: {e:#}"),
    })
}

pub async fn status(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::BankRead).await?;
    let connections = bank_feed::list(&state.pool, company_id).await?;
    Ok(Json(json!({
        "konfigurert": state.bank_feed.is_some(),
        "tillegg": bank_feed::addon_active(&state.pool, company_id, ADDON).await?,
        "koblinger": connections.iter().map(|c| json!({
            "id": c.id,
            "account_number": c.account_number,
            "bank": c.bank,
            "kontonummer": c.bban,
            "status": c.status,
            "gyldig_til": c.valid_until.map(|d| d.date_naive().to_string()),
            "sist_hentet": c.last_synced_at,
            "siste_feil": c.last_error,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn banks(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::BankRead).await?;
    let banks = provider(&state)?.banks(COUNTRY).await.map_err(feed_error)?;
    Ok(Json(json!({
        "banker": banks.iter().map(|b| json!({ "name": b.name, "country": b.country })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct ConnectRequest {
    account_number: String,
    bank: String,
    /// The bank account being connected; defaults to the company's own
    /// (Firmaopplysninger).
    kontonummer: Option<String>,
}

/// Sends the user to the bank. Also the renewal: connecting an account that
/// already has a feed replaces it when the user comes back.
pub async fn connect(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<ConnectRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::BankReconcile).await?;
    let client = provider(&state)?;
    require_addon(&state, company_id).await?;
    // Configuration, never the request's Host: the bank only redirects to
    // the URL registered on our application.
    let Some(base) = &state.portal_base else {
        return Err(ApiError::BadRequest(
            "PORTAL_BASE_URL er ikke satt, så banken har ingen adresse å sende deg tilbake til"
                .into(),
        ));
    };
    let stated = match request
        .kontonummer
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        Some(k) => k.to_string(),
        None => regnmed_db::company_settings(&state.pool, company_id)
            .await?
            .bank_account
            .ok_or_else(|| {
                ApiError::BadRequest(
                    "oppgi kontonummeret som skal kobles til (selskapet har ikke registrert ett)"
                        .into(),
                )
            })?,
    };
    let bban = norwegian_bban(&stated)
        .filter(|b| regnmed_core::pain001::is_valid_account_number(b))
        .ok_or_else(|| {
            ApiError::BadRequest("kontonummeret er ikke et gyldig norsk kontonummer".into())
        })?;
    let bank = client
        .banks(COUNTRY)
        .await
        .map_err(feed_error)?
        .into_iter()
        .find(|b| b.name == request.bank)
        .ok_or_else(|| ApiError::BadRequest("ukjent bank".into()))?;
    let days = match bank.maximum_consent_validity / 86_400 {
        0 => MAX_CONSENT_DAYS,
        d => d.min(MAX_CONSENT_DAYS),
    };
    let id = bank_feed::create_pending(
        &state.pool,
        company_id,
        request.account_number.trim(),
        &bank.name,
        &bban,
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    let url = client
        .start_auth(
            &bank.name,
            &bank.country,
            &id.to_string(),
            &format!("{}/bank/callback", base.trim_end_matches('/')),
            Utc::now() + chrono::Duration::days(days),
        )
        .await
        .map_err(feed_error)?;
    Ok(Json(json!({ "url": url })))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Where the bank sends the browser back. Open by necessity (a redirect
/// carries no token) and therefore does NOTHING but hand the parameters to
/// the portal, which completes the connection as the signed-in user. The
/// code is worthless without our application key.
pub async fn callback(State(state): State<AppState>, Query(q): Query<CallbackQuery>) -> Redirect {
    let pending = match q.state.as_deref().and_then(|s| Uuid::parse_str(s).ok()) {
        Some(id) => bank_feed::pending(&state.pool, id)
            .await
            .ok()
            .flatten()
            .map(|p| (id, p)),
        None => None,
    };
    let Some((id, pending)) = pending else {
        return Redirect::to("/");
    };
    let mut params = reqwest::Url::parse("http://x/").expect("static url");
    {
        let mut pairs = params.query_pairs_mut();
        match (&q.code, &q.error) {
            (Some(code), None) => {
                pairs.append_pair("bankkode", code);
                pairs.append_pair("banktilstand", &id.to_string());
            }
            _ => {
                pairs.append_pair(
                    "bankfeil",
                    q.error_description
                        .as_deref()
                        .or(q.error.as_deref())
                        .unwrap_or("tilkoblingen ble avbrutt"),
                );
            }
        }
    }
    Redirect::to(&format!(
        "/#/c/{}/bank?{}",
        pending.company_id,
        params.query().unwrap_or("")
    ))
}

#[derive(Deserialize)]
pub struct CompleteRequest {
    code: String,
    state: Uuid,
}

/// Turns the code into a session and binds the account. The account must be
/// the one stated at connect, and in NOK.
pub async fn complete(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CompleteRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::BankReconcile).await?;
    let client = provider(&state)?;
    require_addon(&state, company_id).await?;
    let pending = bank_feed::pending(&state.pool, request.state)
        .await?
        .filter(|p| p.company_id == company_id)
        .ok_or_else(|| {
            ApiError::BadRequest("tilkoblingen finnes ikke lenger; start på nytt".into())
        })?;
    let session = client
        .create_session(&request.code)
        .await
        .map_err(feed_error)?;
    let account = session.accounts.iter().find(|a| {
        [a.iban.as_deref(), a.other.as_deref()]
            .into_iter()
            .flatten()
            .filter_map(norwegian_bban)
            .any(|b| b == pending.bban)
    });
    let Some(account) = account else {
        // The consent is for something else: do not keep it.
        let _ = client.end_session(&session.session_id).await;
        return Err(ApiError::BadRequest(format!(
            "banken ga ikke tilgang til konto {}. Velg den kontoen når du godkjenner hos banken.",
            pending.bban
        )));
    };
    if account.currency != "NOK" {
        let _ = client.end_session(&session.session_id).await;
        return Err(ApiError::BadRequest(
            "bare kontoer i norske kroner kan kobles til".into(),
        ));
    }
    bank_feed::activate(
        &state.pool,
        request.state,
        company_id,
        &session.session_id,
        &account.uid,
        session.valid_until,
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({
        "id": request.state,
        "status": "aktiv",
        "gyldig_til": session.valid_until.date_naive().to_string(),
    })))
}

/// «Hent nå»: fetch and import. The same function a scheduled fetch will call.
pub async fn sync(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, connection_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::BankReconcile).await?;
    let client = provider(&state)?;
    require_addon(&state, company_id).await?;
    let connection = bank_feed::live(&state.pool, company_id, connection_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let summary = sync_connection(&state.pool, client, &connection).await?;
    Ok(Json(
        json!({ "nye": summary.new, "auto_matchet": summary.auto_matched }),
    ))
}

/// One fetch for one connection. A consent the bank says is gone ends the
/// connection (the user must connect again); any other failure is noted on
/// the row and reported, and the next fetch tries again.
pub async fn sync_connection(
    pool: &sqlx::PgPool,
    client: &Client,
    connection: &bank_feed::Live,
) -> Result<bank_feed::SyncSummary, ApiError> {
    if connection.valid_until <= Utc::now() {
        return Err(feed_error(FeedError::ConsentGone("expired".into())));
    }
    let today = Utc::now().date_naive();
    let from = bank_feed::fetch_from(pool, connection.account_id, today).await?;
    match client
        .transactions(&connection.external_account_id, from)
        .await
    {
        Ok(fetched) => Ok(bank_feed::record_fetch(pool, connection, &fetched).await?),
        Err(e) => {
            if matches!(e, FeedError::ConsentGone(_)) {
                bank_feed::end(
                    pool,
                    connection.company_id,
                    connection.id,
                    "samtykke utlopt hos banken",
                )
                .await?;
            } else {
                bank_feed::note_error(pool, connection.id, &e.to_string()).await?;
            }
            Err(feed_error(e))
        }
    }
}

pub async fn end(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, connection_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require(&state, person.person_id, company_id, Right::BankReconcile).await?;
    // Ending works without the add-on and without a provider: nobody is
    // ever stuck with a connection they cannot remove.
    if let (Some(client), Some(live)) = (
        state.bank_feed.as_deref(),
        bank_feed::live(&state.pool, company_id, connection_id).await?,
    ) {
        let _ = client.end_session(&live.session_id).await;
    }
    bank_feed::end(
        &state.pool,
        company_id,
        connection_id,
        "koblet fra av bruker",
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
