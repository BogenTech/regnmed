//! Tenancy and authorization resolution.
//!
//! The OIDC token proves identity; everything about *what a person may do*
//! is resolved here, from regnmed's own tables: direct company memberships
//! plus firm memberships routed through active engagements (oppdrag).

use anyhow::Result;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// One company a person may act for, and with what access.
#[derive(Debug, Clone)]
pub struct CompanyAccess {
    pub company_id: Uuid,
    pub orgnr: String,
    pub name: String,
    /// 'admin', 'bokforing' or 'les'.
    pub access: String,
    /// How the access is granted: 'direkte' or the granting firm's name.
    pub via: String,
}

/// Just-in-time provisioning: called on every authenticated request, keyed
/// by the IdP's stable subject claim. Name/email are refreshed from the
/// token but never blanked.
pub async fn ensure_person(
    pool: &PgPool,
    oidc_sub: &str,
    name: Option<&str>,
    email: Option<&str>,
) -> Result<Uuid> {
    let row = sqlx::query(
        "insert into person (id, oidc_sub, name, email) values ($1, $2, $3, $4)
         on conflict (oidc_sub) do update
             set name  = coalesce(excluded.name, person.name),
                 email = coalesce(excluded.email, person.email)
         returning id",
    )
    .bind(Uuid::now_v7())
    .bind(oidc_sub)
    .bind(name)
    .bind(email)
    .fetch_one(pool)
    .await?;
    Ok(row.get("id"))
}

pub async fn ensure_firm(pool: &PgPool, orgnr: &str, name: &str, kind: &str) -> Result<Uuid> {
    sqlx::query(
        "insert into firm (id, orgnr, name, kind) values ($1, $2, $3, $4)
         on conflict (orgnr) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(orgnr)
    .bind(name)
    .bind(kind)
    .execute(pool)
    .await?;
    let row = sqlx::query("select id from firm where orgnr = $1")
        .bind(orgnr)
        .fetch_one(pool)
        .await?;
    Ok(row.get("id"))
}

pub async fn ensure_firm_member(
    pool: &PgPool,
    firm_id: Uuid,
    person_id: Uuid,
    role: &str,
) -> Result<()> {
    sqlx::query(
        "insert into firm_member (firm_id, person_id, role) values ($1, $2, $3)
         on conflict (firm_id, person_id) do nothing",
    )
    .bind(firm_id)
    .bind(person_id)
    .bind(role)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn ensure_company_member(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    role: &str,
) -> Result<()> {
    sqlx::query(
        "insert into company_member (company_id, person_id, role) values ($1, $2, $3)
         on conflict (company_id, person_id) do nothing",
    )
    .bind(company_id)
    .bind(person_id)
    .bind(role)
    .execute(pool)
    .await?;
    Ok(())
}

/// Opens an engagement unless one is already open for the same
/// firm/company/kind (enforced by the partial unique index).
pub async fn ensure_engagement(
    pool: &PgPool,
    firm_id: Uuid,
    company_id: Uuid,
    kind: &str,
) -> Result<()> {
    sqlx::query(
        "insert into engagement (id, firm_id, company_id, kind) values ($1, $2, $3, $4)
         on conflict (firm_id, company_id, kind) where valid_to is null do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(firm_id)
    .bind(company_id)
    .bind(kind)
    .execute(pool)
    .await?;
    Ok(())
}

/// The person's access to one specific company ('admin', 'bokforing' or
/// 'les'), or `None` — resolved through the same paths as
/// [`company_access_for_person`]. The API's per-company guard.
pub async fn company_access(
    pool: &PgPool,
    person_id: Uuid,
    company_id: Uuid,
) -> Result<Option<String>> {
    let access = company_access_for_person(pool, person_id)
        .await?
        .into_iter()
        .filter(|a| a.company_id == company_id)
        // 'admin' > 'bokforing' > 'revisor' > 'les' > 'ansatt'.
        //
        // NOTE: this is a heuristic that only works while the roles form
        // a ladder, and `ansatt` does not — it can write things `les`
        // cannot. Use [`company_roles`] when the answer decides access;
        // this exists for display and for callers that genuinely want a
        // single name.
        .max_by_key(|a| match a.access.as_str() {
            "admin" => 4,
            "bokforing" => 3,
            "revisor" => 2,
            "les" => 1,
            _ => 0,
        });
    Ok(access.map(|a| a.access))
}

/// **Every** role a person has in one company, one per route in.
///
/// The access guard unions the rettigheter from these rather than picking
/// the "strongest" role: since `ansatt` arrived (#54) the roles are no
/// longer a ladder, and someone who is an employee of the company AND
/// comes in through an oppdrag would lose the right to log their own
/// hours if we picked just one.
/// picked just one.
pub async fn company_roles(
    pool: &PgPool,
    person_id: Uuid,
    company_id: Uuid,
) -> Result<Vec<String>> {
    let mut roller: Vec<String> = company_access_for_person(pool, person_id)
        .await?
        .into_iter()
        .filter(|a| a.company_id == company_id)
        .map(|a| a.access)
        .collect();
    roller.sort();
    roller.dedup();
    Ok(roller)
}

/// Every company the person may act for: direct memberships, plus firm
/// memberships routed through engagements that are active today. An
/// engagement of kind 'regnskap' grants 'bokforing'; 'revisjon' grants
/// 'revisor' — read-only like 'les', but with the payroll access the
/// audit needs (#55). A person can appear once per access path — the caller (or UI)
/// decides how to merge.
///
/// **No route here crosses a company boundary.** All three are confined
/// to one company in the data model itself, and there is no wildcard.
/// The platform roles (docs/auth.md §8, `plattform.rs`) deliberately do
/// NOT flow through this resolver: they grant no company access, ever.
/// That is a decision, not an accident (#57), and it is pinned in tests:
/// an admin in one company is a complete stranger in another, and a
/// platform systemadmin is a complete stranger to every ledger.
pub async fn company_access_for_person(
    pool: &PgPool,
    person_id: Uuid,
) -> Result<Vec<CompanyAccess>> {
    let rows = sqlx::query(
        "select c.id as company_id, c.orgnr, c.name, cm.role as access, 'direkte' as via
         from company_member cm
         join company c on c.id = cm.company_id
         where cm.person_id = $1 and cm.active

         union all

         select c.id as company_id, c.orgnr, c.name,
                case e.kind when 'regnskap' then 'bokforing' else 'revisor' end as access,
                f.name as via
         from firm_member fm
         join firm f on f.id = fm.firm_id
         join engagement e on e.firm_id = fm.firm_id
         join company c on c.id = e.company_id
         where fm.person_id = $1 and fm.active
           and e.valid_from <= current_date
           -- valid_to is the date the oppdrag ended (exclusive): ending an
           -- engagement revokes access immediately, on the same day.
           and (e.valid_to is null or e.valid_to > current_date)

         union all

         -- Maskin-tilgang (docs/integrations.md, #45): en integrasjon er
         -- en prinsipal som alle andre, og tilgangen er et grant med
         -- samme eksklusive valid_to — tilbakekalling virker straks.
         select c.id as company_id, c.orgnr, c.name, g.access, 'integrasjon' as via
         from integration i
         join integration_grant g on g.integration_id = i.id
         join company c on c.id = g.company_id
         where i.person_id = $1
           and g.valid_from <= current_date
           and (g.valid_to is null or g.valid_to > current_date)

         order by name, via",
    )
    .bind(person_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| CompanyAccess {
            company_id: r.get("company_id"),
            orgnr: r.get("orgnr"),
            name: r.get("name"),
            access: r.get("access"),
            via: r.get("via"),
        })
        .collect())
}
