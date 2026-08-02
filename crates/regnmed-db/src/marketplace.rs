//! Marketplace onboarding: companies from Enhetsregisteret, firms with
//! verified autorisasjon. Registry lookups happen in the API layer
//! (regnmed-gov); this module persists the results.

use anyhow::{Context, Result, bail, ensure};
use sqlx::PgPool;
use uuid::Uuid;

use crate::ledger::{create_company, ensure_account, ensure_journal, find_company_by_orgnr};
use crate::reskontro::set_account_reskontro;
use crate::tenancy::ensure_company_member;

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

/// Creates a firm whose autorisasjon has been confirmed against
/// Finanstilsynets register, records the verification, and makes the
/// creator the firm's admin — all in one transaction.
///
/// An orgnr that is already a firm is refused (#78): registration used
/// to be idempotent, which silently made the SECOND person to register
/// the same byrå a co-admin of the existing one. Joining an existing
/// byrå is an invitation decision its admins make, not a side effect.
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
    const ALLEREDE: &str =
        "byrået er allerede registrert i regnmed — be en administrator der om å invitere deg";
    let mut tx = pool.begin().await?;
    let existing: Option<Uuid> = sqlx::query_scalar("select id from firm where orgnr = $1")
        .bind(orgnr)
        .fetch_optional(&mut *tx)
        .await?;
    ensure!(existing.is_none(), "{ALLEREDE}");

    let firm_id = Uuid::now_v7();
    let res = sqlx::query(
        "insert into firm (id, orgnr, name, kind, autorisasjon_verified_at, autorisasjon_ref)
         values ($1, $2, $3, $4, now(), $5)",
    )
    .bind(firm_id)
    .bind(orgnr)
    .bind(registry_name)
    .bind(kind)
    .bind(autorisasjon_ref)
    .execute(&mut *tx)
    .await;
    if let Err(e) = res {
        // Two simultaneous registrations: the loser of the unique race
        // gets the same answer as if it had seen the row.
        if let Some(db) = e.as_database_error() {
            if db.code().as_deref() == Some("23505") {
                bail!("{ALLEREDE}");
            }
        }
        return Err(e.into());
    }
    sqlx::query("insert into firm_member (firm_id, person_id, role) values ($1, $2, 'admin')")
        .bind(firm_id)
        .bind(person_id)
        .execute(&mut *tx)
        .await?;
    crate::byramedlemmer::logg(
        &mut tx,
        firm_id,
        person_id,
        "lagt_til",
        None,
        Some("admin"),
        Some(person_id),
        "registrering",
    )
    .await?;
    tx.commit().await?;
    Ok(firm_id)
}
