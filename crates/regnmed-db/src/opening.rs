//! Manual åpningsbalanse — the wizard path for companies whose old
//! system cannot produce SAF-T (or that are new enough to have none):
//! the administrator enters per-account balances, which become one
//! `Åpningsbalanse` voucher through the normal posting path.
//!
//! Same honesty rules as the SAF-T route (crate::saft_import): only
//! into an empty ledger, the balances must sum to zero, and lines on
//! reskontro-flagged accounts defer the flag (warned, never hidden) —
//! an opening total has no party breakdown yet.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::PgPool;
use uuid::Uuid;

use crate::ledger::{PostedVoucher, post_voucher_in};

pub struct OpeningReport {
    pub posted: PostedVoucher,
    pub warnings: Vec<String>,
}

pub async fn post_opening_balance(
    pool: &PgPool,
    company_id: Uuid,
    date: NaiveDate,
    lines: &[(String, i64)],
    created_by: &str,
) -> Result<OpeningReport> {
    ensure!(
        lines.len() >= 2,
        "en åpningsbalanse trenger minst to linjer"
    );
    let sum: i64 = lines.iter().map(|(_, ore)| ore).sum();
    ensure!(
        sum == 0,
        "åpningsbalansen summerer til {sum} øre — debet og kredit må gå i null"
    );

    let mut tx = pool.begin().await?;
    let last_seq: i64 =
        sqlx::query_scalar("select last_seq from chain_head where company_id = $1 for update")
            .bind(company_id)
            .fetch_optional(&mut *tx)
            .await?
            .context("company has no chain head")?;
    ensure!(
        last_seq == 0,
        "hovedboken har allerede {last_seq} bilag — åpningsbalanse legges bare inn i et tomt selskap"
    );

    sqlx::query(
        "insert into journal (id, company_id, code, name) values ($1, $2, 'GL', 'Hovedbok')
         on conflict (company_id, code) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .execute(&mut *tx)
    .await?;

    // Opening totals carry no party breakdown; reskontro flags on
    // touched accounts are deferred with a warning, like the SAF-T path.
    let mut warnings = Vec::new();
    for (account, _) in lines {
        let flagged: Option<String> = sqlx::query_scalar(
            "update account set reskontro_kind = null
             where company_id = $1 and number = $2 and reskontro_kind is not null
             returning number",
        )
        .bind(company_id)
        .bind(account)
        .fetch_optional(&mut *tx)
        .await?;
        if flagged.is_some() {
            warnings.push(format!(
                "konto {account}: reskontro-flagg utsatt — åpningsbalansen har ingen partfordeling"
            ));
        }
    }

    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: "Åpningsbalanse".into(),
        reverses: None,
        entries: lines
            .iter()
            .map(|(account, ore)| EntryDraft {
                account_number: account.clone(),
                amount: Ore(*ore),
                vat_code: None,
                description: None,
                party_no: None,
                avdeling: None,
                prosjekt: None,
            })
            .collect(),
    };
    let posted = post_voucher_in(&mut tx, company_id, &draft, created_by).await?;
    tx.commit().await?;
    Ok(OpeningReport { posted, warnings })
}
