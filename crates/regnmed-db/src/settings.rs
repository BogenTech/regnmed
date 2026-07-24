//! Company kontaktinfo for salgsdokumenter (migration 0019, issue #32):
//! address, kontonummer and selskapsform — editable master data used by
//! the invoice PDF; never part of any hash (the stored PDF is the
//! evidence of what a document said when it was issued).

use anyhow::{Result, ensure};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct CompanySettings {
    pub name: String,
    pub orgnr: String,
    pub address: Option<String>,
    pub bank_account: Option<String>,
    pub orgform: Option<String>,
}

pub async fn company_settings(pool: &PgPool, company_id: Uuid) -> Result<CompanySettings> {
    let row = sqlx::query(
        "select name, orgnr, address, bank_account, orgform from company where id = $1",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await?;
    Ok(CompanySettings {
        name: row.get("name"),
        orgnr: row.get("orgnr"),
        address: row.get("address"),
        bank_account: row.get("bank_account"),
        orgform: row.get("orgform"),
    })
}

/// Updates only the fields passed as Some; empty strings clear a field.
pub async fn update_company_settings(
    pool: &PgPool,
    company_id: Uuid,
    address: Option<&str>,
    bank_account: Option<&str>,
    orgform: Option<&str>,
) -> Result<()> {
    fn clear(v: Option<&str>) -> Option<&str> {
        v.map(str::trim).filter(|s| !s.is_empty())
    }
    let updated = sqlx::query(
        "update company set
             address = case when $2 then $3 else address end,
             bank_account = case when $4 then $5 else bank_account end,
             orgform = case when $6 then $7 else orgform end
         where id = $1",
    )
    .bind(company_id)
    .bind(address.is_some())
    .bind(clear(address))
    .bind(bank_account.is_some())
    .bind(clear(bank_account))
    .bind(orgform.is_some())
    .bind(clear(orgform))
    .execute(pool)
    .await?;
    ensure!(updated.rows_affected() == 1, "no such company");
    Ok(())
}

/// Party kontaktinfo (address for the PDF, e-mail for utsendelse).
pub async fn update_party_contact(
    pool: &PgPool,
    company_id: Uuid,
    party_id: Uuid,
    address: Option<&str>,
    email: Option<&str>,
) -> Result<()> {
    if let Some(email) = email {
        let email = email.trim();
        ensure!(
            email.is_empty() || email.contains('@'),
            "ugyldig e-postadresse"
        );
    }
    fn clear(v: Option<&str>) -> Option<&str> {
        v.map(str::trim).filter(|s| !s.is_empty())
    }
    let updated = sqlx::query(
        "update party set
             address = case when $3 then $4 else address end,
             email = case when $5 then $6 else email end
         where id = $1 and company_id = $2",
    )
    .bind(party_id)
    .bind(company_id)
    .bind(address.is_some())
    .bind(clear(address))
    .bind(email.is_some())
    .bind(clear(email))
    .execute(pool)
    .await?;
    ensure!(updated.rows_affected() == 1, "no such party");
    Ok(())
}

/// The invoice's stored PDF (the attachment written at issue time).
pub async fn invoice_pdf_attachment_id(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
) -> Result<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "select a.id from attachment a
         join invoice i on i.voucher_id = a.voucher_id
         where i.id = $1 and i.company_id = $2 and a.content_type = 'application/pdf'
         order by a.created_at limit 1",
    )
    .bind(invoice_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?)
}
