//! Marketplace onboarding: companies from Enhetsregisteret, firms with
//! verified autorisasjon. Registry lookups happen in the API layer
//! (regnmed-gov); this module persists the results.

use anyhow::{Context, Result, ensure};
use sqlx::PgPool;
use uuid::Uuid;

use crate::ledger::{create_company, ensure_account, ensure_journal, find_company_by_orgnr};
use crate::reskontro::set_account_reskontro;
use crate::tenancy::{ensure_company_member, ensure_firm, ensure_firm_member};

/// Starter kontoplan (NS 4102 core) every onboarded company gets:
/// enough to invoice, pay, and reconcile from day one.
const STARTER_ACCOUNTS: &[(&str, &str)] = &[
    ("1500", "Kundefordringer"),
    ("1920", "Bankinnskudd"),
    ("2400", "Leverandørgjeld"),
    ("2700", "Utgående merverdiavgift"),
    ("2710", "Inngående merverdiavgift"),
    ("3000", "Salgsinntekt, avgiftspliktig"),
    ("4300", "Innkjøp av varer for videresalg"),
    ("6300", "Leie lokale"),
    ("6800", "Kontorkostnad"),
    ("7770", "Bank- og kortgebyr"),
];

#[derive(Debug)]
pub struct OnboardedCompany {
    pub company_id: Uuid,
    pub name: String,
    pub seeded_accounts: usize,
}

/// Creates a company from verified registry facts, makes the onboarding
/// person its admin, and seeds journal + starter kontoplan (1500/2400
/// flagged as reskontro). Idempotency: an orgnr can only be onboarded
/// once.
pub async fn onboard_company(
    pool: &PgPool,
    orgnr: &str,
    registry_name: &str,
    person_id: Uuid,
) -> Result<OnboardedCompany> {
    ensure!(
        find_company_by_orgnr(pool, orgnr).await?.is_none(),
        "company {orgnr} is already onboarded"
    );
    let company_id = create_company(pool, orgnr, registry_name)
        .await
        .context("creating company")?;
    ensure_company_member(pool, company_id, person_id, "admin").await?;
    ensure_journal(pool, company_id, "GL", "Hovedbok").await?;
    for (number, name) in STARTER_ACCOUNTS {
        ensure_account(pool, company_id, number, name).await?;
    }
    set_account_reskontro(pool, company_id, "1500", Some("kunde")).await?;
    set_account_reskontro(pool, company_id, "2400", Some("leverandor")).await?;
    Ok(OnboardedCompany {
        company_id,
        name: registry_name.to_string(),
        seeded_accounts: STARTER_ACCOUNTS.len(),
    })
}

/// Creates (or verifies) a firm whose autorisasjon has been confirmed
/// against Finanstilsynets register, records the verification, and makes
/// the creator a firm admin.
pub async fn create_verified_firm(
    pool: &PgPool,
    orgnr: &str,
    registry_name: &str,
    kind: &str,
    autorisasjon_ref: &str,
    person_id: Uuid,
) -> Result<Uuid> {
    ensure!(
        kind == "regnskap" || kind == "revisjon",
        "kind must be 'regnskap' or 'revisjon'"
    );
    let firm_id = ensure_firm(pool, orgnr, registry_name, kind).await?;
    sqlx::query(
        "update firm set autorisasjon_verified_at = now(), autorisasjon_ref = $2
         where id = $1",
    )
    .bind(firm_id)
    .bind(autorisasjon_ref)
    .execute(pool)
    .await?;
    ensure_firm_member(pool, firm_id, person_id, "admin").await?;
    Ok(firm_id)
}
