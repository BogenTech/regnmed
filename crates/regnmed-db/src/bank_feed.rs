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
    pub provider: String,
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
        "select c.id, c.provider, a.number, c.bank_name, c.bban, c.session_id, c.valid_until,
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
                provider: r.get("provider"),
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
    provider: &str,
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
         values ($1,$2,$3,$7,$4,$5,$6)",
    )
    .bind(id)
    .bind(company_id)
    .bind(account_id)
    .bind(bank)
    .bind(bban)
    .bind(by)
    .bind(provider)
    .execute(pool)
    .await?;
    Ok(id)
}

#[derive(Debug)]
pub struct Pending {
    pub company_id: Uuid,
    pub provider: String,
    pub bban: String,
}

/// A connection still waiting for its consent, younger than an hour.
pub async fn pending(pool: &PgPool, id: Uuid) -> Result<Option<Pending>> {
    Ok(sqlx::query(
        "select company_id, provider, bban from bank_connection
         where id = $1 and session_id is null and ended_at is null
           and created_at > now() - interval '1 hour'",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .map(|r| Pending {
        company_id: r.get("company_id"),
        provider: r.get("provider"),
        bban: r.get("bban"),
    }))
}

/// A consent that is no longer ours to keep: the caller ends it at the provider.
#[derive(Debug)]
pub struct Replaced {
    pub provider: String,
    pub session_id: String,
}

/// Completes a pending connection. An earlier live connection for the same
/// ledger account ends in the same transaction (a renewal), so there is
/// never a moment with two feeds into one account. Returns the sessions that
/// were replaced, which must be closed at their provider.
pub async fn activate(
    pool: &PgPool,
    id: Uuid,
    company_id: Uuid,
    session_id: &str,
    external_account_id: &str,
    valid_until: DateTime<Utc>,
) -> Result<Vec<Replaced>> {
    let mut tx = pool.begin().await?;
    let replaced = sqlx::query(
        "update bank_connection set ended_at = now(), end_reason = 'fornyet'
         where ended_at is null and session_id is not null
           and account_id = (select account_id from bank_connection where id = $1)
         returning provider, session_id",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(|r| Replaced {
        provider: r.get("provider"),
        session_id: r.get("session_id"),
    })
    .collect();
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
    Ok(replaced)
}

#[derive(Debug)]
pub struct Live {
    pub id: Uuid,
    pub provider: String,
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
        "select id, provider, company_id, account_id, bban, session_id, external_account_id, valid_until
         from bank_connection
         where id = $1 and company_id = $2 and ended_at is null and session_id is not null",
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .map(|r| Live {
        id: r.get("id"),
        provider: r.get("provider"),
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
/// dedupe table absorbs the overlap). The FIRST sync starts ON the day of the
/// newest statement line already on the account: history uploaded as files is
/// not fetched again, while that last day is — a file exported at noon lacks
/// the afternoon, and `record_fetch` leaves out what the file already has.
/// With nothing on the account it goes back to the start of this fiscal year
/// — the year still to be booked, and the bank lines are its to-do list — but
/// not into history the ledger already holds: the day after the newest
/// imported voucher, or the date of the opening balance. Never later than today.
/// It is the first fetch that reaches back: banks release more than 90 days
/// only shortly after the customer has authenticated.
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
    let from = match (fed, newest) {
        (Some(fed), _) => fed - chrono::Days::new(OVERLAP_DAYS as u64),
        (None, Some(newest)) => newest,
        (None, None) => {
            let year_start =
                regnmed_core::fiscal_year::period(regnmed_core::fiscal_year::of(today))
                    .context("fiscal year out of range")?
                    .0;
            // Imported history (the IMP journal) is the old system's, reconciled
            // there; a manual opening balance summarizes everything before it.
            let history_end: Option<NaiveDate> = sqlx::query_scalar(
                "select max(case when j.code = 'IMP' then v.voucher_date + 1 else v.voucher_date end)
                 from voucher v join journal j on j.id = v.journal_id
                 where v.company_id = (select company_id from account where id = $1)
                   and (j.code = 'IMP' or (v.chain_seq = 1 and v.description = 'Åpningsbalanse'))",
            )
            .bind(account_id)
            .fetch_one(pool)
            .await?;
            history_end.map_or(year_start, |d| d.max(year_start))
        }
    };
    Ok(from.min(today))
}

#[derive(Debug, Serialize)]
pub struct SyncSummary {
    /// Transactions imported as new statement lines.
    pub new: usize,
    pub auto_matched: usize,
}

type Line = regnmed_core::camt053::Camt053Transaction;

/// Stores what a fetch returned. Transactions not seen before become one
/// statement, and their keys are recorded in the SAME transaction, so a
/// failure imports nothing and a retry imports everything exactly once. Two
/// concurrent syncs collide on the key and one rolls back whole.
///
/// Two kinds of fetched transaction never become a line: a zero amount (an
/// information entry; the CSV import skips them too), and one that is already
/// on the account from an uploaded file — same day, same amount, one file
/// line absorbing one fetched transaction. The absorbed ones are recorded as
/// seen, so the next fetch does not consider them again.
pub async fn record_fetch(
    pool: &PgPool,
    connection: &Live,
    fetched: &[FeedTx],
) -> Result<SyncSummary> {
    let mut txs = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    for t in fetched {
        let line = to_transaction(t)
            .map_err(|e| anyhow::anyhow!("banken sendte et beløp som ikke kan leses: {e}"))?;
        if line.amount_ore != 0 {
            txs.push(t.clone());
            lines.push(line);
        }
    }
    let keys = dedupe_keys(&txs);
    let seen: std::collections::HashSet<String> = sqlx::query_scalar(
        "select key from bank_feed_seen where account_id = $1 and key = any($2)",
    )
    .bind(connection.account_id)
    .bind(&keys)
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();
    let fresh: Vec<(String, Line)> = keys
        .into_iter()
        .zip(lines)
        .filter(|(k, _)| !seen.contains(k))
        .collect();

    // What uploaded files already hold for these days, minus what earlier
    // fetches have absorbed: (day, amount) -> (lines left to absorb, a statement).
    let dates: Vec<NaiveDate> = fresh.iter().map(|(_, l)| l.booking_date).collect();
    let mut in_files: std::collections::HashMap<(NaiveDate, i64), (i64, Uuid)> = sqlx::query(
        "select t.booking_date, t.amount_ore, count(*) n, min(s.id::text)::uuid statement_id,
                (select count(*) from bank_feed_seen f
                  where f.account_id = $1 and f.absorbed
                    and f.booking_date = t.booking_date and f.amount_ore = t.amount_ore) absorbed
         from bank_transaction t join bank_statement s on s.id = t.statement_id
         where s.account_id = $1 and s.statement_ref not like 'feed:%'
           and t.booking_date = any($2)
         group by t.booking_date, t.amount_ore",
    )
    .bind(connection.account_id)
    .bind(&dates)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| {
        (
            (r.get("booking_date"), r.get("amount_ore")),
            (
                r.get::<i64, _>("n") - r.get::<i64, _>("absorbed"),
                r.get("statement_id"),
            ),
        )
    })
    .collect();
    let mut absorbed: Vec<(&(String, Line), Uuid)> = Vec::new();
    let mut new: Vec<&(String, Line)> = Vec::new();
    for item in &fresh {
        match in_files.get_mut(&(item.1.booking_date, item.1.amount_ore)) {
            Some((left, statement)) if *left > 0 => {
                *left -= 1;
                absorbed.push((item, *statement));
            }
            _ => new.push(item),
        }
    }

    // Written as one unit. If another fetch of the same account (a scheduled run
    // and «Hent nå» at the same moment) wrote these keys first, this one loses
    // on the primary key, rolls back whole, and has nothing new to report.
    let written: Result<()> = async {
        let mut tx = pool.begin().await?;
        const SEEN: &str = "insert into bank_feed_seen
            (account_id, key, statement_id, booking_date, amount_ore, absorbed)
            values ($1,$2,$3,$4,$5,$6)";
        for ((key, line), statement_id) in &absorbed {
            sqlx::query(SEEN)
                .bind(connection.account_id)
                .bind(key.as_str())
                .bind(statement_id)
                .bind(line.booking_date)
                .bind(line.amount_ore)
                .bind(true)
                .execute(&mut *tx)
                .await?;
        }
        if !new.is_empty() {
            let statement = Camt053Statement {
                statement_ref: format!("feed:{}:{}", connection.id, Uuid::now_v7()),
                iban: None,
                from_date: new.iter().map(|(_, l)| l.booking_date).min(),
                to_date: new.iter().map(|(_, l)| l.booking_date).max(),
                // A feed has no statement balances: absent, never zero (like CSV).
                opening_ore: None,
                closing_ore: None,
                transactions: new
                    .iter()
                    .map(|(_, l)| Line {
                        booking_date: l.booking_date,
                        amount_ore: l.amount_ore,
                        description: l.description.clone(),
                        reference: l.reference.clone(),
                    })
                    .collect(),
            };
            let statement_id = crate::bank::insert_statement_in(
                &mut tx,
                connection.company_id,
                connection.account_id,
                &statement,
                "bankkobling",
            )
            .await?;
            for (key, line) in &new {
                sqlx::query(SEEN)
                    .bind(connection.account_id)
                    .bind(key.as_str())
                    .bind(statement_id)
                    .bind(line.booking_date)
                    .bind(line.amount_ore)
                    .bind(false)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }
    .await;
    if let Err(e) = written {
        let lost_race = e
            .downcast_ref::<sqlx::Error>()
            .and_then(|e| e.as_database_error())
            .and_then(|d| d.code())
            .as_deref()
            == Some("23505");
        if !lost_race {
            return Err(e);
        }
        return Ok(SyncSummary {
            new: 0,
            auto_matched: 0,
        });
    }
    sqlx::query(
        "update bank_connection set last_synced_at = now(), last_error = null where id = $1",
    )
    .bind(connection.id)
    .execute(pool)
    .await?;
    let auto_matched = if new.is_empty() {
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
        new: new.len(),
        auto_matched,
    })
}

/// A company's add-ons with when and why, for the platform console.
pub async fn addon_rows(
    pool: &PgPool,
    company_id: Uuid,
) -> Result<Vec<(String, NaiveDate, String)>> {
    Ok(sqlx::query_as(
        "select addon, valid_from, note from company_addon
         where company_id = $1 and valid_from <= current_date
           and (valid_to is null or valid_to > current_date)
         order by addon",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

/// What the user is told about a provider failure. The provider's own words
/// are for the operator and go to the log, never to the portal.
pub fn user_message(e: &regnmed_gov::bankfeed::FeedError) -> &'static str {
    use regnmed_gov::bankfeed::FeedError;
    match e {
        FeedError::ConsentGone(_) => {
            "Samtykket hos banken er utløpt eller trukket tilbake. Koble til på nytt."
        }
        FeedError::RateLimited => {
            "Banken tillater ikke flere hentinger akkurat nå. Prøv igjen om noen timer."
        }
        FeedError::PeriodRefused => {
            "Banken gir ikke historikk så langt tilbake. Velg en senere dato."
        }
        FeedError::Other(_) => "Banken svarte ikke som ventet. Prøv igjen senere.",
    }
}

/// One fetch for one connection: «Hent nå» in the portal and the scheduled
/// job run exactly this. A consent the bank says is gone ends the connection
/// (the user must connect again); any other failure is noted on the row in
/// the user's language and the next fetch tries again. The error returned is
/// the provider's, for the caller to turn into its own kind of answer.
///
/// `back_to` reaches further back than the usual window (history the first
/// fetch did not cover); it never shortens it. What was already imported is
/// absorbed by the dedupe table, so reaching back is safe to repeat.
pub async fn sync(
    pool: &PgPool,
    client: &dyn regnmed_gov::bankfeed::BankFeed,
    connection: &Live,
    back_to: Option<NaiveDate>,
) -> Result<std::result::Result<SyncSummary, regnmed_gov::bankfeed::FeedError>> {
    use regnmed_gov::bankfeed::FeedError;
    if connection.valid_until <= Utc::now() {
        // The same ending as when the bank says so: the user must connect again.
        end(
            pool,
            connection.company_id,
            connection.id,
            "samtykke utlopt",
        )
        .await?;
        return Ok(Err(FeedError::ConsentGone("expired by date".into())));
    }
    let today = Utc::now().date_naive();
    let usual = fetch_from(pool, connection.account_id, today).await?;
    let from = back_to.map_or(usual, |d| d.min(usual));
    let account = &connection.external_account_id;
    // A bank that will not reach back to the start of the year still gives the
    // last 90 days (PSD2's floor). Only the usual window falls back: a date the
    // user chose is answered as asked.
    let floor = today - chrono::Days::new(90);
    let fetched = match client.transactions(account, from).await {
        Err(FeedError::PeriodRefused) if back_to.is_none() && from < floor => {
            client.transactions(account, floor).await
        }
        r => r,
    };
    match fetched {
        Ok(fetched) => Ok(Ok(record_fetch(pool, connection, &fetched).await?)),
        Err(e) => {
            eprintln!("bank feed: connection {}: {e}", connection.id);
            if matches!(e, FeedError::ConsentGone(_)) {
                end(
                    pool,
                    connection.company_id,
                    connection.id,
                    "samtykke utlopt hos banken",
                )
                .await?;
            } else if !(matches!(e, FeedError::PeriodRefused) && back_to.is_some()) {
                // A refused «Hent eldre» date says nothing about the connection.
                note_error(pool, connection.id, user_message(&e)).await?;
            }
            Ok(Err(e))
        }
    }
}

/// Every completed, not ended connection of a company.
pub async fn live_connections(pool: &PgPool, company_id: Uuid) -> Result<Vec<Live>> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "select id from bank_connection
         where company_id = $1 and ended_at is null and session_id is not null
         order by created_at",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for id in ids {
        if let Some(c) = live(pool, company_id, id).await? {
            out.push(c);
        }
    }
    Ok(out)
}

/// The scheduled fetch for one company (job kind `bank_sync`): every live
/// connection, one after the other. A connection that fails does not fail
/// the job — retrying would only spend the bank's daily fetch allowance —
/// it is reported in `feil`, in the user's language.
pub async fn sync_company(
    pool: &PgPool,
    providers: Option<&regnmed_gov::bankfeed::Providers>,
    company_id: Uuid,
) -> Result<serde_json::Value> {
    let mut summary = (0usize, 0usize, 0usize);
    let mut errors: Vec<String> = Vec::new();
    if !addon_active(pool, company_id, ADDON).await? {
        errors.push("Selskapet har ikke tillegget Bankkobling.".into());
    } else {
        for connection in live_connections(pool, company_id).await? {
            let Some(client) = providers.and_then(|p| p.get(&connection.provider)) else {
                errors.push("Leverandøren bak koblingen er ikke satt opp.".into());
                continue;
            };
            summary.0 += 1;
            match sync(pool, client, &connection, None).await? {
                Ok(s) => {
                    summary.1 += s.new;
                    summary.2 += s.auto_matched;
                }
                Err(e) => errors.push(user_message(&e).into()),
            }
        }
    }
    Ok(serde_json::json!({
        "koblinger": summary.0,
        "nye": summary.1,
        "auto_matchet": summary.2,
        "feil": errors,
    }))
}
