//! Kontoplan management and manual bilagsføring (docs/hovedbok.md).
//!
//! Accounts are master data: the NUMBER is what postings reference and
//! what the hash chain covers, so a number can never change — but names
//! are editable and accounts deactivate rather than delete. The standard
//! catalog (Skatteetaten's grouping list, vendored in regnmed-core) is
//! served alongside so the portal can offer every code a regnskapsfører
//! knows, without seeding 254 rows into every company: an account exists
//! in the company only once someone adds it or posts to it.

use anyhow::{Context, Result, bail, ensure};
use regnmed_core::voucher::VoucherDraft;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::{PostedVoucher, post_voucher_in};

pub struct KontoRow {
    pub number: String,
    pub name: String,
    pub vat_code: Option<String>,
    pub active: bool,
    pub reskontro_kind: Option<String>,
    /// Lifetime balance (SUM of entries, øre) — computed, never stored.
    pub saldo_ore: i64,
    pub posteringer: i64,
}

/// Every account the company has, with computed balances. Accounts with
/// zero postings are included — a freshly added account must be visible,
/// or "add account" looks like it did nothing.
pub async fn list_accounts(pool: &PgPool, company_id: Uuid) -> Result<Vec<KontoRow>> {
    let rows = sqlx::query(
        "select a.number, a.name, a.vat_code, a.active, a.reskontro_kind,
                coalesce(sum(e.amount_ore), 0)::bigint as saldo_ore,
                count(e.id)::bigint as posteringer
         from account a
         left join entry e on e.account_id = a.id
         where a.company_id = $1
         group by a.id
         order by a.number",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| KontoRow {
            number: r.get("number"),
            name: r.get("name"),
            vat_code: r.get("vat_code"),
            active: r.get("active"),
            reskontro_kind: r.get("reskontro_kind"),
            saldo_ore: r.get("saldo_ore"),
            posteringer: r.get("posteringer"),
        })
        .collect())
}

/// Add an account to the company's kontoplan. The name may be the
/// standard one (resolved by the caller) or the company's own — custom
/// accounts are first-class, they just need a name.
pub async fn create_account(
    pool: &PgPool,
    company_id: Uuid,
    number: &str,
    name: &str,
) -> Result<()> {
    ensure!(
        number.len() == 4 && number.chars().all(|c| c.is_ascii_digit()),
        "kontonummer må være fire sifre"
    );
    ensure!(!name.trim().is_empty(), "kontoen må ha et navn");
    let result = sqlx::query(
        "insert into account (id, company_id, number, name) values ($1, $2, $3, $4)
         on conflict (company_id, number) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(number)
    .bind(name.trim())
    .execute(pool)
    .await?;
    ensure!(
        result.rows_affected() == 1,
        "konto {number} finnes allerede i kontoplanen"
    );
    Ok(())
}

/// Rename and/or (de)activate. The number is permanent — it is what the
/// ledger's hash chain references. Deactivating stops NEW postings
/// (post_voucher requires an active account); history is untouched.
pub async fn update_account(
    pool: &PgPool,
    company_id: Uuid,
    number: &str,
    name: Option<&str>,
    active: Option<bool>,
) -> Result<()> {
    if let Some(n) = name {
        ensure!(!n.trim().is_empty(), "kontoen må ha et navn");
    }
    let result = sqlx::query(
        "update account set name = coalesce($3, name), active = coalesce($4, active)
         where company_id = $1 and number = $2",
    )
    .bind(company_id)
    .bind(number)
    .bind(name.map(str::trim))
    .bind(active)
    .execute(pool)
    .await?;
    ensure!(result.rows_affected() == 1, "ingen konto {number}");
    Ok(())
}

/// Post a free-form manual voucher. Everything post_voucher already
/// enforces applies (period lock, active accounts, dims, reskontro,
/// double entry); what this adds is the attestering boundary: when the
/// company runs an active attestering policy and the voucher is at or
/// over the beløpsgrense, a manual posting would BYPASS the intern
/// kontroll (#47) — attestation targets inbox documents. Fail closed and
/// point at the innboks instead of quietly opening a side door.
pub async fn post_manual_voucher(
    pool: &PgPool,
    company_id: Uuid,
    draft: &VoucherDraft,
    created_by: &str,
) -> Result<PostedVoucher> {
    let mut tx = pool.begin().await?;
    if let Some(policy) = crate::attestering::current_policy(&mut *tx, company_id).await?
        && policy.aktiv
    {
        let debetsum: i64 = draft.entries.iter().map(|e| e.amount.0.max(0)).sum();
        if policy
            .belopsgrense_ore
            .is_none_or(|grense| debetsum >= grense)
        {
            bail!(
                "attesteringspolicyen krever attestering for bilag på dette beløpet — \
                 last dokumentet opp i innboksen og bokfør det derfra"
            );
        }
    }
    let posted = post_voucher_in(&mut tx, company_id, draft, created_by)
        .await
        .context("bokføringen feilet")?;
    tx.commit().await?;
    Ok(posted)
}
