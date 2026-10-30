//! Company kontaktinfo for salgsdokumenter (migration 0019, issue #32):
//! address, kontonummer and selskapsform — editable master data used by
//! the invoice PDF; never part of any hash (the stored PDF is the
//! evidence of what a document said when it was issued).

use anyhow::{Result, ensure};
use chrono::NaiveDate;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// The company's registrations as they stood on a given date
/// (migration 0056, #81). Both flags govern påtegninger on the
/// salgsdokument under bokføringsforskriften §5-1-2 and are STORED,
/// never derived from the document being rendered.
#[derive(Debug, Clone, Copy, Default)]
pub struct Registrering {
    pub mva_registrert: bool,
    pub foretaksregistrert: bool,
}

/// Registration status in force on `dato` — newest row at or before it.
///
/// No row at all yields `false`/`false`: we do not claim a registration
/// we hold no evidence for. Every company gets a row from migration
/// 0056, so this is the case of a company created outside the normal
/// paths (tests, fixtures).
pub async fn registrering_on(
    pool: &PgPool,
    company_id: Uuid,
    date: NaiveDate,
) -> Result<Registrering> {
    let row = sqlx::query(
        "select mva_registrert, foretaksregistrert from company_registrering
         where company_id = $1 and valid_from <= $2
         order by valid_from desc limit 1",
    )
    .bind(company_id)
    .bind(date)
    .fetch_optional(pool)
    .await?;
    Ok(row.map_or(Registrering::default(), |r| Registrering {
        mva_registrert: r.get("mva_registrert"),
        foretaksregistrert: r.get("foretaksregistrert"),
    }))
}

/// SAF-T codes that assert nothing inside the VAT system: no treatment
/// on the purchase side, turnover outside the act, no treatment on the
/// income side. These are what an unregistered company's lines carry —
/// `abonnement::utgaende_mva` picks `7` for exactly that reason — so
/// they are the only codes the guard below lets through.
///
/// Code `5` is deliberately NOT here. It is zero-rated too, but it
/// means a REGISTERED seller's exempt sale (mval. kap. 6), which is a
/// claim an unregistered company cannot make even at rate 0: the
/// hovedbok and the SAF-T would say something untrue about what
/// happened.
const UTENFOR_MVA: [&str; 3] = ["0", "6", "7"];

/// Whether this code requires the company to be mva-registered.
pub fn krever_mva_registrering(code: &str) -> bool {
    !UTENFOR_MVA.contains(&code)
}

/// Refuses a VAT code the company has no right to use on `dato`.
///
/// Output VAT may only be charged by a registered seller (mval. §15-11
/// second paragraph) and input VAT only deducted by one (§8-1). A code
/// on a document from an unregistered company is therefore an error,
/// never a silent zero: the amounts the user asked for and the amounts
/// posted would differ without anyone being told.
///
/// Dated on the DOCUMENT, never today — a company that registers
/// mid-year must leave its earlier documents as they were, the same
/// rule the påtegning follows (#81).
///
/// Callers that DERIVE a code instead of taking one from a person use
/// code 7 and rate 0 rather than this guard; see
/// `abonnement::utgaende_mva`.
pub async fn krev_mva_registrert(pool: &PgPool, company_id: Uuid, date: NaiveDate) -> Result<()> {
    let reg = registrering_on(pool, company_id, date).await?;
    ensure!(
        reg.mva_registrert,
        "selskapet var ikke mva-registrert {date} og kan ikke bruke mva-koder — \
         fjern mva-koden, eller oppdater registreringsstatusen under \
         firmaopplysninger hvis selskapet faktisk er registrert"
    );
    Ok(())
}

/// Records a registration observation. Insert-only: a correction is a
/// NEW row, and re-recording the same day replaces that day's row
/// (the observation is about the day, not about the moment we asked).
pub async fn record_registrering(
    pool: &PgPool,
    company_id: Uuid,
    valid_from: NaiveDate,
    reg: Registrering,
    kilde: &str,
    notat: Option<&str>,
    recorded_by: &str,
) -> Result<()> {
    ensure!(
        matches!(kilde, "brreg" | "manuell" | "migrert"),
        "ukjent kilde «{kilde}»"
    );
    sqlx::query(
        "insert into company_registrering
             (id, company_id, valid_from, mva_registrert, foretaksregistrert,
              kilde, notat, recorded_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         on conflict (company_id, valid_from) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(valid_from)
    .bind(reg.mva_registrert)
    .bind(reg.foretaksregistrert)
    .bind(kilde)
    .bind(notat)
    .bind(recorded_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// The recorded history, newest first — the audit trail behind a
/// påtegning on an old salgsdokument.
pub async fn registrering_history(
    pool: &PgPool,
    company_id: Uuid,
) -> Result<Vec<(NaiveDate, Registrering, String, Option<String>)>> {
    let rows = sqlx::query(
        "select valid_from, mva_registrert, foretaksregistrert, kilde, notat
         from company_registrering where company_id = $1
         order by valid_from desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get("valid_from"),
                Registrering {
                    mva_registrert: r.get("mva_registrert"),
                    foretaksregistrert: r.get("foretaksregistrert"),
                },
                r.get("kilde"),
                r.get("notat"),
            )
        })
        .collect())
}

#[derive(Debug)]
pub struct CompanySettings {
    pub name: String,
    pub orgnr: String,
    pub address: Option<String>,
    pub bank_account: Option<String>,
    pub orgform: Option<String>,
    /// Reply-to for utsendelser — the company's address, never regnmed's.
    pub email: Option<String>,
    /// Has the company a KID/OCR agreement with its bank? Governs
    /// whether a KID is printed at all (#104).
    pub kid_avtale: bool,
}

pub async fn company_settings(pool: &PgPool, company_id: Uuid) -> Result<CompanySettings> {
    let row = sqlx::query(
        "select name, orgnr, address, bank_account, orgform, email, kid_avtale
         from company where id = $1",
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
        email: row.get("email"),
        kid_avtale: row.get("kid_avtale"),
    })
}

/// Updates only the fields passed as Some; empty strings clear a field.
pub async fn update_company_settings(
    pool: &PgPool,
    company_id: Uuid,
    address: Option<&str>,
    bank_account: Option<&str>,
    orgform: Option<&str>,
    email: Option<&str>,
    kid_avtale: Option<bool>,
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
        "update company set
             address = case when $2 then $3 else address end,
             kid_avtale = coalesce($10, kid_avtale),
             bank_account = case when $4 then $5 else bank_account end,
             orgform = case when $6 then $7 else orgform end,
             email = case when $8 then $9 else email end
         where id = $1",
    )
    .bind(company_id)
    .bind(address.is_some())
    .bind(clear(address))
    .bind(bank_account.is_some())
    .bind(clear(bank_account))
    .bind(orgform.is_some())
    .bind(clear(orgform))
    .bind(email.is_some())
    .bind(clear(email))
    .bind(kid_avtale)
    .execute(pool)
    .await?;
    ensure!(updated.rows_affected() == 1, "no such company");
    Ok(())
}

/// Party kontaktinfo (address for the PDF, e-mail for utsendelse,
/// kontonummer for remittering — validated MOD11 before storage).
pub async fn update_party_contact(
    pool: &PgPool,
    company_id: Uuid,
    party_id: Uuid,
    address: Option<&str>,
    email: Option<&str>,
    bank_account: Option<&str>,
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
    let normalized_account = match clear(bank_account) {
        Some(account) => {
            let normalized = regnmed_core::pain001::normalize_account_number(account);
            ensure!(
                regnmed_core::pain001::is_valid_account_number(&normalized),
                "ugyldig kontonummer (11 siffer, MOD11)"
            );
            Some(normalized)
        }
        None => None,
    };
    let updated = sqlx::query(
        "update party set
             address = case when $3 then $4 else address end,
             email = case when $5 then $6 else email end,
             bank_account = case when $7 then $8 else bank_account end
         where id = $1 and company_id = $2",
    )
    .bind(party_id)
    .bind(company_id)
    .bind(address.is_some())
    .bind(clear(address))
    .bind(email.is_some())
    .bind(clear(email))
    .bind(bank_account.is_some())
    .bind(normalized_account)
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
