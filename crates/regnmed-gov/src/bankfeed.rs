//! The seam between regnmed and whoever fetches bank transactions for it
//! (docs/bank.md, «Bankkobling»). regnmed is not tied to one provider: each
//! one implements `BankFeed`, the API holds the configured ones in priority
//! order, and a connection remembers which provider it was made through.
//!
//! The shape is the redirect consent every PSD2 route has: start at the
//! bank, come back with a code, get a session with accounts, read booked
//! transactions until the consent runs out. A provider that works
//! differently (a bank file channel, say) is a different trait, not a
//! stretched version of this one.

use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, NaiveDate, Utc};
use regnmed_core::bankfeed::FeedTx;

/// Why a call failed, kept apart where the caller must act differently.
#[derive(Debug)]
pub enum FeedError {
    /// The consent is gone (expired, revoked at the bank, or closed): the
    /// user must connect again. Not a fault.
    ConsentGone(String),
    /// The bank refuses more fetches for now; try again later.
    RateLimited,
    /// The bank does not give history that far back. Not a fault of the
    /// connection: a later start date works.
    PeriodRefused,
    Other(anyhow::Error),
}

impl std::fmt::Display for FeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeedError::ConsentGone(why) => write!(f, "consent is gone: {why}"),
            FeedError::RateLimited => write!(f, "the bank is rate limiting"),
            FeedError::PeriodRefused => write!(f, "the bank refuses that transaction period"),
            FeedError::Other(e) => write!(f, "{e:#}"),
        }
    }
}

impl From<anyhow::Error> for FeedError {
    fn from(e: anyhow::Error) -> Self {
        FeedError::Other(e)
    }
}

#[derive(Debug, Clone)]
pub struct Bank {
    pub name: String,
    pub country: String,
    /// Longest consent the bank allows, in seconds (0 = not stated).
    pub maximum_consent_validity: i64,
}

#[derive(Debug, Clone)]
pub struct FeedAccount {
    /// The provider's id for the account, valid while the session is.
    pub uid: String,
    pub iban: Option<String>,
    /// A domestic account number (BBAN) when the bank gives no IBAN.
    pub other: Option<String>,
    pub currency: String,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: String,
    pub valid_until: DateTime<Utc>,
    pub accounts: Vec<FeedAccount>,
}

pub struct AuthRequest<'a> {
    pub bank: &'a str,
    pub country: &'a str,
    /// Comes back untouched on the redirect.
    pub state: &'a str,
    pub redirect_url: &'a str,
    pub valid_until: DateTime<Utc>,
}

pub type Fut<'a, T> = Pin<Box<dyn Future<Output = Result<T, FeedError>> + Send + 'a>>;

pub trait BankFeed: Send + Sync {
    /// Stored on the connection (`bank_connection.provider`); permanent.
    fn id(&self) -> &'static str;
    /// Banks that offer business accounts in `country`.
    fn banks<'a>(&'a self, country: &'a str) -> Fut<'a, Vec<Bank>>;
    /// Starts the consent; returns the URL to send the user to.
    fn start_auth<'a>(&'a self, request: &'a AuthRequest<'a>) -> Fut<'a, String>;
    /// Turns the code from the redirect into a session.
    fn create_session<'a>(&'a self, code: &'a str) -> Fut<'a, Session>;
    /// BOOKED transactions from `from` (inclusive) until today, whole days.
    fn transactions<'a>(&'a self, account_uid: &'a str, from: NaiveDate) -> Fut<'a, Vec<FeedTx>>;
    /// Ends the consent at the provider.
    fn end_session<'a>(&'a self, session_id: &'a str) -> Fut<'a, ()>;
}

/// One bank as offered through one provider.
#[derive(Debug, Clone)]
pub struct Offer {
    pub provider: &'static str,
    pub bank: Bank,
}

/// The list of banks changes rarely; a connect should not depend twice on
/// the provider being up.
const BANKS_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

/// The configured providers, in priority order. Built once per process, by
/// the API (the consent flow, «Hent nå») and by the worker (the scheduled
/// fetch), from the same environment.
pub struct Providers {
    providers: Vec<std::sync::Arc<dyn BankFeed>>,
    banks: std::sync::Mutex<Option<(std::time::Instant, Vec<Offer>)>>,
}

impl Providers {
    /// None when no provider is given: the feature then does not exist.
    pub fn new(providers: Vec<std::sync::Arc<dyn BankFeed>>) -> Option<std::sync::Arc<Self>> {
        (!providers.is_empty()).then(|| {
            std::sync::Arc::new(Self {
                providers,
                banks: std::sync::Mutex::new(None),
            })
        })
    }

    /// Every provider the environment configures. Each is "all of its
    /// settings or none"; a key is PEM text in a variable or a file outside
    /// the repo and the image (docs/secrets.md).
    pub fn from_env() -> anyhow::Result<Option<std::sync::Arc<Self>>> {
        use anyhow::Context;
        let mut providers: Vec<std::sync::Arc<dyn BankFeed>> = Vec::new();
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let key = match (var("ENABLEBANKING_KEY"), var("ENABLEBANKING_KEY_FILE")) {
            (Some(pem), _) => Some(pem.into_bytes()),
            (None, Some(file)) => Some(
                std::fs::read(&file)
                    .with_context(|| format!("reading ENABLEBANKING_KEY_FILE {file}"))?,
            ),
            (None, None) => None,
        };
        match (var("ENABLEBANKING_APP_ID"), key) {
            (Some(app_id), Some(pem)) => {
                providers.push(std::sync::Arc::new(crate::enablebanking::Client::new(
                    &app_id,
                    &pem,
                    var("ENABLEBANKING_API_BASE").as_deref(),
                )?))
            }
            (None, None) => {}
            _ => anyhow::bail!(
                "set ENABLEBANKING_APP_ID together with ENABLEBANKING_KEY or ENABLEBANKING_KEY_FILE, or none of them"
            ),
        }
        Ok(Self::new(providers))
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.providers.iter().map(|p| p.id()).collect()
    }

    pub fn get(&self, id: &str) -> Option<&dyn BankFeed> {
        self.providers.iter().find(|p| p.id() == id).map(|p| &**p)
    }

    /// Every bank on offer in `country`, first provider first. One provider
    /// being down does not hide the others' banks; all failing is an error.
    pub async fn offers(&self, country: &str) -> Result<Vec<Offer>, FeedError> {
        if let Some((at, offers)) = self.banks.lock().unwrap().as_ref()
            && at.elapsed() < BANKS_TTL
        {
            return Ok(offers.clone());
        }
        let mut offers = Vec::new();
        let mut failure = None;
        for p in &self.providers {
            match p.banks(country).await {
                Ok(banks) => offers.extend(banks.into_iter().map(|bank| Offer {
                    provider: p.id(),
                    bank,
                })),
                Err(e) => {
                    eprintln!("bank feed: provider {} could not list banks: {e}", p.id());
                    failure = Some(e);
                }
            }
        }
        match failure {
            Some(e) if offers.is_empty() => Err(e),
            // A partial list is not cached: the missing provider is asked again next time.
            Some(_) => Ok(offers),
            None => {
                *self.banks.lock().unwrap() = Some((std::time::Instant::now(), offers.clone()));
                Ok(offers)
            }
        }
    }
}
