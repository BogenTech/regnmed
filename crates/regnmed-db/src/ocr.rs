//! OCR giro persistence: batch import (idempotent per oppdrag) and the
//! payment listing the web shows until reskontro can apply payments to
//! invoices automatically.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::ocr::OcrFile;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct OcrImportSummary {
    pub batches: usize,
    pub payments: usize,
    pub sum_ore: i64,
    pub kid_invalid: usize,
}

pub async fn import_ocr_file(
    pool: &PgPool,
    company_id: Uuid,
    account_number: &str,
    file: &OcrFile,
    imported_by: &str,
) -> Result<OcrImportSummary> {
    let account_id: Uuid =
        sqlx::query("select id from account where company_id = $1 and number = $2 and active")
            .bind(company_id)
            .bind(account_number)
            .fetch_optional(pool)
            .await?
            .with_context(|| format!("no active account {account_number} for this company"))?
            .get("id");

    let mut summary = OcrImportSummary {
        batches: 0,
        payments: 0,
        sum_ore: 0,
        kid_invalid: 0,
    };
    let mut tx = pool.begin().await?;
    for assignment in &file.assignments {
        let batch_id = Uuid::now_v7();
        let inserted = sqlx::query(
            "insert into ocr_batch (id, company_id, account_id, transmission_number,
                                    assignment_number, agreement_id, bank_account, imported_by)
             values ($1, $2, $3, $4, $5, $6, $7, $8)
             on conflict (company_id, transmission_number, assignment_number) do nothing",
        )
        .bind(batch_id)
        .bind(company_id)
        .bind(account_id)
        .bind(&file.transmission_number)
        .bind(&assignment.assignment_number)
        .bind(&assignment.agreement_id)
        .bind(&assignment.bank_account)
        .bind(imported_by)
        .execute(&mut *tx)
        .await?;
        ensure!(
            inserted.rows_affected() == 1,
            "oppdrag {} in forsendelse {} is already imported",
            assignment.assignment_number,
            file.transmission_number
        );

        for payment in &assignment.payments {
            sqlx::query(
                "insert into ocr_payment (id, batch_id, transaction_number, payment_date,
                                          amount_ore, kid, kid_valid, transaction_type,
                                          bank_reference, debit_account)
                 values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(Uuid::now_v7())
            .bind(batch_id)
            .bind(&payment.transaction_number)
            .bind(payment.date)
            .bind(payment.amount_ore)
            .bind(&payment.kid)
            .bind(payment.kid_valid)
            .bind(&payment.transaction_type)
            .bind(&payment.bank_reference)
            .bind(&payment.debit_account)
            .execute(&mut *tx)
            .await?;
            summary.payments += 1;
            summary.sum_ore += payment.amount_ore;
            if !payment.kid_valid {
                summary.kid_invalid += 1;
            }
        }
        summary.batches += 1;
    }
    tx.commit().await?;
    Ok(summary)
}

#[derive(Debug)]
pub struct OcrPaymentRow {
    pub id: Uuid,
    pub payment_date: NaiveDate,
    pub amount_ore: i64,
    pub kid: String,
    pub kid_valid: bool,
    pub transaction_type: String,
    pub bank_reference: Option<String>,
    pub account_number: String,
}

pub async fn list_ocr_payments(
    pool: &PgPool,
    company_id: Uuid,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> Result<Vec<OcrPaymentRow>> {
    let rows = sqlx::query(
        "select p.id, p.payment_date, p.amount_ore, p.kid, p.kid_valid,
                p.transaction_type, p.bank_reference, a.number as account_number
         from ocr_payment p
         join ocr_batch b on b.id = p.batch_id
         join account a on a.id = b.account_id
         where b.company_id = $1
           and ($2::date is null or p.payment_date >= $2)
           and ($3::date is null or p.payment_date <= $3)
         order by p.payment_date, p.transaction_number",
    )
    .bind(company_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| OcrPaymentRow {
            id: r.get("id"),
            payment_date: r.get("payment_date"),
            amount_ore: r.get("amount_ore"),
            kid: r.get("kid"),
            kid_valid: r.get("kid_valid"),
            transaction_type: r.get("transaction_type"),
            bank_reference: r.get("bank_reference"),
            account_number: r.get("account_number"),
        })
        .collect())
}
