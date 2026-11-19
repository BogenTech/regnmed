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
//!
//! regnmed is not tied to one provider. `Providers` (regnmed-gov) holds the configured ones
//! in priority order, a bank offered by several is taken from the first, and
//! every connection remembers which provider it was made through.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use chrono::Utc;
use regnmed_core::bankfeed::norwegian_bban;
use regnmed_db::bank_feed::{self, ADDON};
use regnmed_gov::bankfeed::{AuthRequest, BankFeed, FeedError, Providers};
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
fn feeds(state: &AppState) -> Result<&Providers, ApiError> {
    state.bank_feed.as_deref().ok_or_else(|| {
        ApiError::BadRequest("bankkobling er ikke satt opp på denne installasjonen".into())
    })
}

fn provider<'a>(feeds: &'a Providers, id: &str) -> Result<&'a dyn BankFeed, ApiError> {
    feeds.get(id).ok_or_else(|| {
        ApiError::BadRequest(
            "koblingen ble laget gjennom en leverandør som ikke lenger er satt opp; koble til på nytt".into(),
        )
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

fn feed_error(e: FeedError) -> ApiError {
    eprintln!("bank feed: {e}");
    ApiError::BadRequest(bank_feed::user_message(&e).into())
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
            "leverandor": c.provider,
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
    let offers = feeds(&state)?.offers(COUNTRY).await.map_err(feed_error)?;
    Ok(Json(json!({
        "banker": offers.iter().map(|o| json!({
            "name": o.bank.name, "country": o.bank.country, "provider": o.provider,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct ConnectRequest {
    account_number: String,
    bank: String,
    /// Which provider to go through; without it, the first one offering the bank.
    provider: Option<String>,
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
    let feeds = feeds(&state)?;
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
    let offer = feeds
        .offers(COUNTRY)
        .await
        .map_err(feed_error)?
        .into_iter()
        .find(|o| {
            o.bank.name == request.bank
                && request.provider.as_deref().is_none_or(|p| p == o.provider)
        })
        .ok_or_else(|| ApiError::BadRequest("ukjent bank".into()))?;
    let days = match offer.bank.maximum_consent_validity / 86_400 {
        0 => MAX_CONSENT_DAYS,
        d => d.min(MAX_CONSENT_DAYS),
    };
    let id = bank_feed::create_pending(
        &state.pool,
        company_id,
        request.account_number.trim(),
        offer.provider,
        &offer.bank.name,
        &bban,
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    let url = provider(feeds, offer.provider)?
        .start_auth(&AuthRequest {
            bank: &offer.bank.name,
            country: &offer.bank.country,
            state: &id.to_string(),
            redirect_url: &format!("{}/bank/callback", base.trim_end_matches('/')),
            valid_until: Utc::now() + chrono::Duration::days(days),
        })
        .await
        .map_err(feed_error)?;
    Ok(Json(json!({ "url": url })))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// Where the bank sends the browser back. Open by necessity (a redirect
/// carries no token) and therefore does NOTHING but hand the parameters to
/// the portal, which completes the connection as the signed-in user. The
/// code is worthless without our application key. An error is passed on as
/// a fixed text, never the query's own words: this address is open.
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
                pairs.append_pair("bankfeil", "Tilkoblingen ble avbrutt hos banken.");
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
/// the one stated at connect, and in NOK. A session that is not kept is
/// closed at the provider, and so is the one a renewal replaces: we hold no
/// consent we do not use.
pub async fn complete(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CompleteRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::BankReconcile).await?;
    let feeds = feeds(&state)?;
    require_addon(&state, company_id).await?;
    let pending = bank_feed::pending(&state.pool, request.state)
        .await?
        .filter(|p| p.company_id == company_id)
        .ok_or_else(|| {
            ApiError::BadRequest("tilkoblingen finnes ikke lenger; start på nytt".into())
        })?;
    let client = provider(feeds, &pending.provider)?;
    let session = client
        .create_session(&request.code)
        .await
        .map_err(feed_error)?;
    let refusal = match session.accounts.iter().find(|a| {
        [a.iban.as_deref(), a.other.as_deref()]
            .into_iter()
            .flatten()
            .filter_map(norwegian_bban)
            .any(|b| b == pending.bban)
    }) {
        None => format!(
            "banken ga ikke tilgang til konto {}. Velg den kontoen når du godkjenner hos banken.",
            pending.bban
        ),
        Some(account) if account.currency != "NOK" => {
            "bare kontoer i norske kroner kan kobles til".to_string()
        }
        Some(account) => match bank_feed::activate(
            &state.pool,
            request.state,
            company_id,
            &session.session_id,
            &account.uid,
            session.valid_until,
        )
        .await
        {
            Ok(replaced) => {
                // From now on the fetch also runs by itself (docs/jobs.md,
                // «Tidsplaner»). An existing schedule is left as the company set it.
                regnmed_db::schedule::ensure(
                    &state.pool,
                    company_id,
                    regnmed_db::job::Kind::BankSync,
                    person.display(),
                )
                .await?;
                for old in replaced {
                    if let Some(p) = feeds.get(&old.provider)
                        && let Err(e) = p.end_session(&old.session_id).await
                    {
                        eprintln!(
                            "bank feed: could not close the replaced session at {}: {e}",
                            old.provider
                        );
                    }
                }
                return Ok(Json(json!({
                    "id": request.state,
                    "status": "aktiv",
                    "gyldig_til": session.valid_until.date_naive().to_string(),
                })));
            }
            Err(e) => format!("{e:#}"),
        },
    };
    // The session is not kept, so it is not left open at the provider either.
    if let Err(e) = client.end_session(&session.session_id).await {
        eprintln!("bank feed: could not close an unused session: {e}");
    }
    Err(ApiError::BadRequest(refusal))
}

#[derive(Deserialize)]
pub struct SyncQuery {
    /// Reach back to this day: history the usual window does not cover.
    fra: Option<chrono::NaiveDate>,
}

/// «Hent nå»: fetch and import. The same function a scheduled fetch will call.
/// `?fra=` fetches older history once; the bank decides how far back it goes.
pub async fn sync(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, connection_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<SyncQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::BankReconcile).await?;
    let feeds = feeds(&state)?;
    require_addon(&state, company_id).await?;
    let connection = bank_feed::live(&state.pool, company_id, connection_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let client = provider(feeds, &connection.provider)?;
    let summary = bank_feed::sync(&state.pool, client, &connection, q.fra)
        .await?
        .map_err(|e| ApiError::BadRequest(bank_feed::user_message(&e).into()))?;
    Ok(Json(
        json!({ "nye": summary.new, "auto_matchet": summary.auto_matched }),
    ))
}

pub async fn end(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, connection_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require(&state, person.person_id, company_id, Right::BankReconcile).await?;
    // Ending works without the add-on and without a provider: nobody is
    // ever stuck with a connection they cannot remove.
    if let Some(live) = bank_feed::live(&state.pool, company_id, connection_id).await?
        && let Some(client) = state
            .bank_feed
            .as_deref()
            .and_then(|f| f.get(&live.provider))
        && let Err(e) = client.end_session(&live.session_id).await
    {
        eprintln!(
            "bank feed: could not close the session at {}: {e}",
            live.provider
        );
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
