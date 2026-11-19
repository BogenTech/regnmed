//! Direct bank connection (migration 0096, docs/bank.md tier 2): the
//! add-on entitlement, the consent rows and the import of what a provider
//! returned. The provider client is `regnmed-gov`; this crate only stores.
//! What arrives becomes an ordinary bank statement, matched by the same
//! engine as an uploaded file.

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, NaiveDate, Utc};
use regnmed_core::bankfeed::{FeedTx, dedupe_keys, to_transaction};
use regnmed_core::camt053::Camt053Statement;
use serde::Serialize;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// The add-on slug (subscription_plan, kind `addon`).
pub const ADDON: &str = "bank";
/// History fetched on the very first sync of an account with no statements.
const FIRST_SYNC_DAYS: i64 = 90;
/// Later syncs re-read this many days: banks book late, and the dedupe
/// table makes the overlap free.
const OVERLAP_DAYS: i64 = 10;

pub async fn addon_active(pool: &PgPool, company_id: Uuid, addon: &str) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "select exists(select 1 from company_addon
                       where company_id = $1 and addon = $2 and valid_from <= current_date
                         and (valid_to is null or valid_to > current_date))",
    )
    .bind(company_id)
    .bind(addon)
    .fetch_one(pool)
    .await?)
}

/// The add-ons a company has today.
pub async fn active_addons(pool: &PgPool, company_id: Uuid) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "select addon from company_addon
         where company_id = $1 and valid_from <= current_date
           and (valid_to is null or valid_to > current_date)
         order by addon",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

/// Gives a company an add-on from today. The add-on must exist as one
/// (kind `addon`); whether it is publicly "available" is not checked here:
/// a pilot is granted before the add-on is for sale.
pub async fn grant_addon(
    pool: &PgPool,
    company_id: Uuid,
    addon: &str,
    note: &str,
    by: &str,
) -> Result<()> {
    ensure!(!note.trim().is_empty(), "tillegget må ha en referanse");
    ensure!(
        crate::plans::existing_kind(pool, addon).await?.as_deref() == Some("addon"),
        "ukjent tilleggstjeneste «{addon}»"
    );
    sqlx::query(
        "insert into company_addon (company_id, addon, note, created_by) values ($1,$2,$3,$4)",
    )
    .bind(company_id)
    .bind(addon)
    .bind(note.trim())
    .bind(by)
    .execute(pool)
    .await
    .context("selskapet har allerede dette tillegget")?;
    Ok(())
}

/// Ends the open add-on row. The shortest coverage is one day (EXCLUSIVE
/// `valid_to`), like `abonnement`.
pub async fn end_addon(pool: &PgPool, company_id: Uuid, addon: &str) -> Result<()> {
    let n = sqlx::query(
        "update company_addon set valid_to = greatest(current_date, valid_from + 1)
         where company_id = $1 and addon = $2 and valid_to is null",
    )
    .bind(company_id)
    .bind(addon)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(n == 1, "selskapet har ikke dette tillegget");
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ConnectionRow {
    pub id: Uuid,
    pub account_number: String,
    pub bank: String,
    pub bban: String,
    /// `venter` (sent to the bank, not back yet), `aktiv`, `utlopt`.
    pub status: &'static str,
    pub valid_until: Option<DateTime<Utc>>,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// Live connections, plus the ones still waiting for the user to come back
/// from the bank (for an hour; an abandoned attempt then drops out).
pub async fn list(pool: &PgPool, company_id: Uuid) -> Result<Vec<ConnectionRow>> {
    let rows = sqlx::query(
        "select c.id, a.number, c.bank_name, c.bban, c.session_id, c.valid_until,
                c.last_synced_at, c.last_error
         from bank_connection c join account a on a.id = c.account_id
         where c.company_id = $1 and c.ended_at is null
           and (c.session_id is not null or c.created_at > now() - interval '1 hour')
         order by a.number, c.created_at desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let now = Utc::now();
    Ok(rows
        .iter()
        .map(|r| {
            let valid_until: Option<DateTime<Utc>> = r.get("valid_until");
            ConnectionRow {
                id: r.get("id"),
                account_number: r.get("number"),
                bank: r.get("bank_name"),
                bban: r.get("bban"),
                status: match (r.get::<Option<String>, _>("session_id"), valid_until) {
                    (None, _) => "venter",
                    (Some(_), Some(until)) if until <= now => "utlopt",
                    (Some(_), _) => "aktiv",
                },
                valid_until,
                last_synced_at: r.get("last_synced_at"),
                last_error: r.get("last_error"),
            }
        })
        .collect())
}

/// Records that the user is being sent to the bank. The id is the `state`.
pub async fn create_pending(
    pool: &PgPool,
    company_id: Uuid,
    account_number: &str,
    bank: &str,
    bban: &str,
    by: &str,
) -> Result<Uuid> {
    let account_id: Uuid = sqlx::query_scalar(
        "select id from account where company_id = $1 and number = $2 and active",
    )
    .bind(company_id)
    .bind(account_number)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("selskapet har ingen aktiv konto {account_number}"))?;
    let id = Uuid::new_v4();
    sqlx::query(
        "insert into bank_connection (id, company_id, account_id, provider, bank_name, bban, created_by)
         values ($1,$2,$3,'enablebanking',$4,$5,$6)",
    )
    .bind(id)
    .bind(company_id)
    .bind(account_id)
    .bind(bank)
    .bind(bban)
    .bind(by)
    .execute(pool)
    .await?;
    Ok(id)
}

#[derive(Debug)]
pub struct Pending {
    pub company_id: Uuid,
    pub bban: String,
}

/// A connection still waiting for its consent, younger than an hour.
pub async fn pending(pool: &PgPool, id: Uuid) -> Result<Option<Pending>> {
    Ok(sqlx::query(
        "select company_id, bban from bank_connection
         where id = $1 and session_id is null and ended_at is null
           and created_at > now() - interval '1 hour'",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .map(|r| Pending {
        company_id: r.get("company_id"),
        bban: r.get("bban"),
    }))
}

/// Completes a pending connection. An earlier live connection for the same
/// ledger account ends in the same transaction (a renewal), so there is
/// never a moment with two feeds into one account.
pub async fn activate(
    pool: &PgPool,
    id: Uuid,
    company_id: Uuid,
    session_id: &str,
    external_account_id: &str,
    valid_until: DateTime<Utc>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "update bank_connection set ended_at = now(), end_reason = 'fornyet'
         where ended_at is null and session_id is not null
           and account_id = (select account_id from bank_connection where id = $1)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let n = sqlx::query(
        "update bank_connection set session_id = $3, external_account_id = $4, valid_until = $5
         where id = $1 and company_id = $2 and session_id is null and ended_at is null",
    )
    .bind(id)
    .bind(company_id)
    .bind(session_id)
    .bind(external_account_id)
    .bind(valid_until)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    ensure!(
        n == 1,
        "tilkoblingen finnes ikke, eller er allerede fullført"
    );
    tx.commit().await?;
    Ok(())
}

#[derive(Debug)]
pub struct Live {
    pub id: Uuid,
    pub company_id: Uuid,
    pub account_id: Uuid,
    pub bban: String,
    pub session_id: String,
    pub external_account_id: String,
    pub valid_until: DateTime<Utc>,
}

/// A completed, not ended connection of this company.
pub async fn live(pool: &PgPool, company_id: Uuid, id: Uuid) -> Result<Option<Live>> {
    Ok(sqlx::query(
        "select id, company_id, account_id, bban, session_id, external_account_id, valid_until
         from bank_connection
         where id = $1 and company_id = $2 and ended_at is null and session_id is not null",
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .map(|r| Live {
        id: r.get("id"),
        company_id: r.get("company_id"),
        account_id: r.get("account_id"),
        bban: r.get("bban"),
        session_id: r.get("session_id"),
        external_account_id: r.get("external_account_id"),
        valid_until: r.get("valid_until"),
    }))
}

/// Ends a connection (also a pending one). Idempotent on an ended row.
pub async fn end(pool: &PgPool, company_id: Uuid, id: Uuid, reason: &str) -> Result<()> {
    sqlx::query(
        "update bank_connection set ended_at = now(), end_reason = $3
         where id = $1 and company_id = $2 and ended_at is null",
    )
    .bind(id)
    .bind(company_id)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn note_error(pool: &PgPool, id: Uuid, error: &str) -> Result<()> {
    sqlx::query("update bank_connection set last_error = $2 where id = $1")
        .bind(id)
        .bind(error)
        .execute(pool)
        .await?;
    Ok(())
}

/// Where the next fetch starts. A later sync re-reads the last days (the
/// dedupe table absorbs the overlap). The FIRST sync starts the day after
/// the newest statement line already on the account, so history that was
/// uploaded as files is not fetched again; with nothing there it goes back
/// a fixed number of days.
pub async fn fetch_from(pool: &PgPool, account_id: Uuid, today: NaiveDate) -> Result<NaiveDate> {
    let r = sqlx::query(
        "select max(t.booking_date) filter (where s.statement_ref like 'feed:%') fed,
                max(t.booking_date) newest
         from bank_transaction t join bank_statement s on s.id = t.statement_id
         where s.account_id = $1",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await?;
    let fed: Option<NaiveDate> = r.get("fed");
    let newest: Option<NaiveDate> = r.get("newest");
    Ok(match (fed, newest) {
        (Some(fed), _) => fed - chrono::Days::new(OVERLAP_DAYS as u64),
        (None, Some(newest)) => newest + chrono::Days::new(1),
        (None, None) => today - chrono::Days::new(FIRST_SYNC_DAYS as u64),
    })
}

#[derive(Debug, Serialize)]
pub struct SyncSummary {
    /// Transactions not seen before.
    pub new: usize,
    pub auto_matched: usize,
}

/// Stores what a fetch returned: the transactions not seen before become
/// one statement, and their keys are recorded in the SAME transaction, so
/// a failure imports nothing and a retry imports everything exactly once.
/// Two concurrent syncs collide on the key and one rolls back whole.
pub async fn record_fetch(
    pool: &PgPool,
    connection: &Live,
    fetched: &[FeedTx],
) -> Result<SyncSummary> {
    let keys = dedupe_keys(fetched);
    let seen: Vec<String> = sqlx::query_scalar(
        "select key from bank_feed_seen where account_id = $1 and key = any($2)",
    )
    .bind(connection.account_id)
    .bind(&keys)
    .fetch_all(pool)
    .await?;
    let fresh: Vec<(&String, &FeedTx)> = keys
        .iter()
        .zip(fetched)
        .filter(|(k, _)| !seen.contains(k))
        .collect();
    if !fresh.is_empty() {
        let transactions = fresh
            .iter()
            .map(|(_, t)| to_transaction(t))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("banken sendte et beløp som ikke kan leses: {e}"))?;
        let statement = Camt053Statement {
            statement_ref: format!("feed:{}:{}", connection.id, Uuid::now_v7()),
            iban: None,
            from_date: transactions.iter().map(|t| t.booking_date).min(),
            to_date: transactions.iter().map(|t| t.booking_date).max(),
            // A feed has no statement balances: absent, never zero (like CSV).
            opening_ore: None,
            closing_ore: None,
            transactions,
        };
        let mut tx = pool.begin().await?;
        let statement_id = crate::bank::insert_statement_in(
            &mut tx,
            connection.company_id,
            connection.account_id,
            &statement,
            "bankkobling",
        )
        .await?;
        for (key, _) in &fresh {
            sqlx::query(
                "insert into bank_feed_seen (account_id, key, statement_id) values ($1,$2,$3)",
            )
            .bind(connection.account_id)
            .bind(key.as_str())
            .bind(statement_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
    }
    sqlx::query(
        "update bank_connection set last_synced_at = now(), last_error = null where id = $1",
    )
    .bind(connection.id)
    .execute(pool)
    .await?;
    let auto_matched = if fresh.is_empty() {
        0
    } else {
        crate::bank::auto_match(
            pool,
            connection.company_id,
            connection.account_id,
            "bankkobling",
        )
        .await?
    };
    Ok(SyncSummary {
        new: fresh.len(),
        auto_matched,
    })
}
