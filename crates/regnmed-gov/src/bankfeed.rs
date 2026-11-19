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
    Other(anyhow::Error),
}

impl std::fmt::Display for FeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeedError::ConsentGone(why) => write!(f, "consent is gone: {why}"),
            FeedError::RateLimited => write!(f, "the bank is rate limiting"),
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
