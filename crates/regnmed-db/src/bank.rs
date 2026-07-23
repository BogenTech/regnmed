//! Bank reconciliation persistence: statement import (idempotent on the
//! bank's statement id), auto-matching via the pure engine in
//! `regnmed_core::bank`, manual match/unmatch, and the reconciliation
//! status — where "unmatched" is always computed from the absence of a
//! match row, never stored state.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::bank::{BankTx, OpenEntry, propose_matches};
use regnmed_core::camt053::Camt053Statement;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Matching window: bank booking date vs voucher date.
const MATCH_WINDOW_DAYS: i64 = 5;

#[derive(Debug)]
pub struct ImportSummary {
    pub statement_id: Uuid,
    pub transactions: usize,
    pub auto_matched: usize,
}

/// Imports a parsed camt.053 statement for a company's bank account and
/// auto-matches its transactions against open ledger entries.
pub async fn import_statement(
    pool: &PgPool,
    company_id: Uuid,
    account_number: &str,
    statement: &Camt053Statement,
    imported_by: &str,
) -> Result<ImportSummary> {
    ensure!(
        !statement.statement_ref.is_empty(),
        "statement has no id (Stmt/Id)"
    );
    let account_id: Uuid =
        sqlx::query("select id from account where company_id = $1 and number = $2 and active")
            .bind(company_id)
            .bind(account_number)
            .fetch_optional(pool)
            .await?
            .with_context(|| format!("no active account {account_number} for this company"))?
            .get("id");

    let mut tx = pool.begin().await?;
    let statement_id = Uuid::now_v7();
    let inserted = sqlx::query(
        "insert into bank_statement (id, company_id, account_id, statement_ref, iban,
                                     from_date, to_date, opening_ore, closing_ore, imported_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         on conflict (company_id, statement_ref) do nothing",
    )
    .bind(statement_id)
    .bind(company_id)
    .bind(account_id)
    .bind(&statement.statement_ref)
    .bind(&statement.iban)
    .bind(statement.from_date)
    .bind(statement.to_date)
    .bind(statement.opening_ore)
    .bind(statement.closing_ore)
    .bind(imported_by)
    .execute(&mut *tx)
    .await?;
    ensure!(
        inserted.rows_affected() == 1,
        "statement {} is already imported",
        statement.statement_ref
    );

    for transaction in &statement.transactions {
        sqlx::query(
            "insert into bank_transaction (id, statement_id, booking_date, amount_ore,
                                           description, reference)
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(statement_id)
        .bind(transaction.booking_date)
        .bind(transaction.amount_ore)
        .bind(&transaction.description)
        .bind(&transaction.reference)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    let auto_matched = auto_match(pool, company_id, account_id, imported_by).await?;
    Ok(ImportSummary {
        statement_id,
        transactions: statement.transactions.len(),
        auto_matched,
    })
}

/// Runs the matching engine over everything currently unmatched on the
/// account and records the proposals as 'auto' matches.
async fn auto_match(
    pool: &PgPool,
    company_id: Uuid,
    account_id: Uuid,
    matched_by: &str,
) -> Result<usize> {
    let bank_txs: Vec<BankTx> = unmatched_bank_rows(pool, company_id, Some(account_id))
        .await?
        .into_iter()
        .map(|r| BankTx {
            id: r.id,
            booking_date: r.booking_date,
            amount_ore: r.amount_ore,
        })
        .collect();

    let entries: Vec<OpenEntry> = sqlx::query(
        "select e.id, v.voucher_date, e.amount_ore
         from entry e
         join voucher v on v.id = e.voucher_id
         where v.company_id = $1 and e.account_id = $2
           and not exists (select 1 from bank_match m where m.entry_id = e.id)",
    )
    .bind(company_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| OpenEntry {
        entry_id: r.get("id"),
        date: r.get("voucher_date"),
        amount_ore: r.get("amount_ore"),
    })
    .collect();

    let proposals = propose_matches(&bank_txs, &entries, MATCH_WINDOW_DAYS);
    for proposal in &proposals {
        sqlx::query(
            "insert into bank_match (bank_transaction_id, entry_id, method, matched_by)
             values ($1, $2, 'auto', $3)",
        )
        .bind(proposal.bank_tx_id)
        .bind(proposal.entry_id)
        .bind(matched_by)
        .execute(pool)
        .await?;
    }
    Ok(proposals.len())
}

/// Manual match, guarded: both sides must belong to the company (and the
/// entry to the same account as the bank transaction's statement); the
/// unique constraints reject double-matching.
pub async fn manual_match(
    pool: &PgPool,
    company_id: Uuid,
    bank_transaction_id: Uuid,
    entry_id: Uuid,
    matched_by: &str,
) -> Result<()> {
    let valid: bool = sqlx::query_scalar(
        "select exists (
             select 1
             from bank_transaction t
             join bank_statement s on s.id = t.statement_id
             join entry e on e.id = $3 and e.account_id = s.account_id
             join voucher v on v.id = e.voucher_id and v.company_id = s.company_id
             where t.id = $2 and s.company_id = $1
         )",
    )
    .bind(company_id)
    .bind(bank_transaction_id)
    .bind(entry_id)
    .fetch_one(pool)
    .await?;
    ensure!(
        valid,
        "bank transaction and entry must belong to the same company and bank account"
    );

    sqlx::query(
        "insert into bank_match (bank_transaction_id, entry_id, method, matched_by)
         values ($1, $2, 'manual', $3)",
    )
    .bind(bank_transaction_id)
    .bind(entry_id)
    .bind(matched_by)
    .execute(pool)
    .await
    .context("already matched — unmatch first")?;
    Ok(())
}

pub async fn unmatch(pool: &PgPool, company_id: Uuid, bank_transaction_id: Uuid) -> Result<()> {
    let removed = sqlx::query(
        "delete from bank_match m
         using bank_transaction t, bank_statement s
         where m.bank_transaction_id = $2
           and t.id = m.bank_transaction_id
           and s.id = t.statement_id and s.company_id = $1",
    )
    .bind(company_id)
    .bind(bank_transaction_id)
    .execute(pool)
    .await?;
    ensure!(removed.rows_affected() == 1, "no such match");
    Ok(())
}

#[derive(Debug)]
pub struct UnmatchedBankRow {
    pub id: Uuid,
    pub booking_date: NaiveDate,
    pub amount_ore: i64,
    pub description: String,
    pub reference: Option<String>,
}

async fn unmatched_bank_rows(
    pool: &PgPool,
    company_id: Uuid,
    account_id: Option<Uuid>,
) -> Result<Vec<UnmatchedBankRow>> {
    let rows = sqlx::query(
        "select t.id, t.booking_date, t.amount_ore, t.description, t.reference
         from bank_transaction t
         join bank_statement s on s.id = t.statement_id
         where s.company_id = $1
           and ($2::uuid is null or s.account_id = $2)
           and not exists (select 1 from bank_match m where m.bank_transaction_id = t.id)
         order by t.booking_date, t.amount_ore",
    )
    .bind(company_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| UnmatchedBankRow {
            id: r.get("id"),
            booking_date: r.get("booking_date"),
            amount_ore: r.get("amount_ore"),
            description: r.get("description"),
            reference: r.get("reference"),
        })
        .collect())
}

#[derive(Debug)]
pub struct UnmatchedEntryRow {
    pub entry_id: Uuid,
    pub voucher_date: NaiveDate,
    pub voucher_label: String,
    pub amount_ore: i64,
    pub description: Option<String>,
}

#[derive(Debug)]
pub struct ReconciliationStatus {
    pub account_number: String,
    pub ledger_balance_ore: i64,
    pub statement_closing_ore: Option<i64>,
    pub statement_to_date: Option<NaiveDate>,
    pub matched_count: i64,
    pub unmatched_bank: Vec<UnmatchedBankRow>,
    pub unmatched_entries: Vec<UnmatchedEntryRow>,
}

pub async fn reconciliation_status(
    pool: &PgPool,
    company_id: Uuid,
    account_number: &str,
) -> Result<ReconciliationStatus> {
    let account_id: Uuid =
        sqlx::query("select id from account where company_id = $1 and number = $2")
            .bind(company_id)
            .bind(account_number)
            .fetch_optional(pool)
            .await?
            .with_context(|| format!("no account {account_number} for this company"))?
            .get("id");

    let ledger_balance_ore: i64 = sqlx::query_scalar(
        "select coalesce(sum(amount_ore), 0)::bigint from entry where account_id = $1",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await?;

    let latest = sqlx::query(
        "select closing_ore, to_date from bank_statement
         where company_id = $1 and account_id = $2
         order by to_date desc nulls last limit 1",
    )
    .bind(company_id)
    .bind(account_id)
    .fetch_optional(pool)
    .await?;

    let matched_count: i64 = sqlx::query_scalar(
        "select count(*) from bank_match m
         join bank_transaction t on t.id = m.bank_transaction_id
         join bank_statement s on s.id = t.statement_id
         where s.company_id = $1 and s.account_id = $2",
    )
    .bind(company_id)
    .bind(account_id)
    .fetch_one(pool)
    .await?;

    let unmatched_entries = sqlx::query(
        "select e.id, v.voucher_date, v.fiscal_year, v.voucher_number,
                e.amount_ore, coalesce(e.description, v.description) as description
         from entry e
         join voucher v on v.id = e.voucher_id
         where v.company_id = $1 and e.account_id = $2
           and not exists (select 1 from bank_match m where m.entry_id = e.id)
         order by v.voucher_date, v.voucher_number",
    )
    .bind(company_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| UnmatchedEntryRow {
        entry_id: r.get("id"),
        voucher_date: r.get("voucher_date"),
        voucher_label: format!(
            "{}-{}",
            r.get::<i32, _>("fiscal_year"),
            r.get::<i64, _>("voucher_number")
        ),
        amount_ore: r.get("amount_ore"),
        description: r.get("description"),
    })
    .collect();

    Ok(ReconciliationStatus {
        account_number: account_number.to_string(),
        ledger_balance_ore,
        // CSV statements carry no balances — absent, never zero.
        statement_closing_ore: latest.as_ref().and_then(|r| r.get("closing_ore")),
        statement_to_date: latest.as_ref().and_then(|r| r.get("to_date")),
        matched_count,
        unmatched_bank: unmatched_bank_rows(pool, company_id, Some(account_id)).await?,
        unmatched_entries,
    })
}
