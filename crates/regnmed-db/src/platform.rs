//! Platform roles: systemadmin and support (docs/auth.md §8).
//!
//! This is the deliberately bounded exception to "no access path crosses a
//! company boundary". A platform role reaches administrative master data —
//! persons, memberships, customer registers — and NOTHING in any company's
//! ledger. The company-scoped guard (`access::krev`) and the access
//! resolution in `tenancy.rs` are untouched by design: a platform member
//! without an ordinary membership is still a stranger to every company's
//! vouchers, balances and reports.
//!
//! The #57 requirements live in the schema (migration 0049): memberships
//! carry a mandatory expiry, every call is logged insert-only, and the log
//! is readable by the company it concerns.

use anyhow::{Result, bail, ensure};
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Roles a platform assignment may put a person into. Built-in roles only:
/// the platform does not know a company's custom role names, and a name
/// that does not exist would grant nothing while looking granted.
pub const SELSKAPSROLLER: [&str; 4] = ["admin", "bokforing", "les", "ansatt"];
pub const BYRAROLLER: [&str; 2] = ["admin", "ansatt"];

#[derive(Debug, Clone)]
pub struct ActivePlatformRole {
    pub role: String,
    pub valid_to: NaiveDate,
}

/// The strongest active platform role for a person, if any. Exclusive
/// `valid_to`, like engagements: revocation takes effect immediately.
pub async fn active_platform_role(
    pool: &PgPool,
    person_id: Uuid,
) -> Result<Option<ActivePlatformRole>> {
    let row = sqlx::query(
        "select rolle, valid_to from platform_member
         where person_id = $1
           and valid_from <= current_date and current_date < valid_to
         order by case rolle when 'systemadmin' then 0 else 1 end, valid_to desc
         limit 1",
    )
    .bind(person_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| ActivePlatformRole {
        role: r.get("rolle"),
        valid_to: r.get("valid_to"),
    }))
}

#[derive(Debug, Clone)]
pub struct PlatformMember {
    pub id: Uuid,
    pub person_id: Uuid,
    pub name: String,
    pub email: Option<String>,
    pub role: String,
    pub valid_from: NaiveDate,
    pub valid_to: NaiveDate,
    pub note: String,
    pub active: bool,
}

pub async fn list_platform_members(pool: &PgPool) -> Result<Vec<PlatformMember>> {
    let rows = sqlx::query(
        "select m.id, m.person_id, coalesce(p.name, p.oidc_sub) as navn, p.email,
                m.rolle, m.valid_from, m.valid_to, m.notat,
                (m.valid_from <= current_date and current_date < m.valid_to) as aktiv
         from platform_member m
         join person p on p.id = m.person_id
         order by aktiv desc, m.valid_to desc, navn",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| PlatformMember {
            id: r.get("id"),
            person_id: r.get("person_id"),
            name: r.get("navn"),
            email: r.get("email"),
            role: r.get("rolle"),
            valid_from: r.get("valid_from"),
            valid_to: r.get("valid_to"),
            note: r.get("notat"),
            active: r.get("aktiv"),
        })
        .collect())
}

/// Grants a platform role. `granted_by` is NULL only from the CLI
/// bootstrap — the first systemadmin cannot arrive through an API that
/// only systemadmins may call.
pub async fn grant_platform_role(
    pool: &PgPool,
    person_id: Uuid,
    role: &str,
    valid_to: NaiveDate,
    note: &str,
    granted_by: Option<Uuid>,
) -> Result<Uuid> {
    ensure!(
        role == "systemadmin" || role == "support",
        "ukjent plattformrolle «{role}» — gyldige er systemadmin og support"
    );
    ensure!(
        valid_to > Utc::now().date_naive(),
        "valid_to må ligge i framtiden — plattformroller er tidsbegrensede"
    );
    ensure!(
        !note.trim().is_empty(),
        "notat er obligatorisk — skriv hvorfor"
    );
    let kind: String = sqlx::query("select kind from person where id = $1")
        .bind(person_id)
        .fetch_optional(pool)
        .await?
        .map(|r| r.get("kind"))
        .unwrap_or_default();
    if kind.is_empty() {
        bail!("ukjent person");
    }
    if kind != "menneske" {
        bail!("plattformroller gis bare til mennesker, ikke integrasjoner");
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "insert into platform_member (id, person_id, rolle, valid_to, notat, granted_by)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(person_id)
    .bind(role)
    .bind(valid_to)
    .bind(note.trim())
    .bind(granted_by)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Ends a platform membership with immediate effect (exclusive valid_to).
/// Rows are never deleted; an already-expired row is left as it stands.
pub async fn end_platform_member(pool: &PgPool, member_id: Uuid) -> Result<()> {
    let updated = sqlx::query(
        "update platform_member
         set valid_to = least(valid_to, greatest(valid_from, current_date))
         where id = $1",
    )
    .bind(member_id)
    .execute(pool)
    .await?;
    ensure!(updated.rows_affected() == 1, "ukjent plattformmedlemskap");
    Ok(())
}

/// Looks a person up by normalised e-mail, for granting roles and
/// assigning memberships. Ambiguity is an error, not a guess.
pub async fn person_by_email(pool: &PgPool, email: &str) -> Result<Option<Uuid>> {
    let norm = email.trim().to_lowercase();
    let rows = sqlx::query("select id from person where lower(email) = $1 and kind = 'menneske'")
        .bind(&norm)
        .fetch_all(pool)
        .await?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(Some(rows[0].get("id"))),
        n => bail!("{n} personer deler adressen {norm} — bruk person-id"),
    }
}

/// One insert per platform call; the middleware in regnmed-api is the only
/// caller, so no endpoint can forget it. Synchronous on purpose — a lost
/// log row would defeat the reason the path is allowed to exist.
pub async fn log_platform_access(
    pool: &PgPool,
    person_id: Uuid,
    role: &str,
    method: &str,
    path: &str,
    company_id: Option<Uuid>,
    firm_id: Option<Uuid>,
) -> Result<()> {
    sqlx::query(
        "insert into platform_access_log
             (person_id, rolle, method, path, company_id, firm_id)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(person_id)
    .bind(role)
    .bind(method)
    .bind(path)
    .bind(company_id)
    .bind(firm_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct PlatformAccessLog {
    pub name: String,
    pub role: String,
    pub method: String,
    pub path: String,
    pub created_at: DateTime<Utc>,
}

async fn access_log(pool: &PgPool, column: &str, id: Uuid) -> Result<Vec<PlatformAccessLog>> {
    // `column` is one of two literals below — never caller input.
    let sql = format!(
        "select coalesce(p.name, p.oidc_sub) as navn, l.rolle, l.method, l.path, l.created_at
         from platform_access_log l
         join person p on p.id = l.person_id
         where l.{column} = $1
         order by l.created_at desc
         limit 200"
    );
    let rows = sqlx::query(&sql).bind(id).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|r| PlatformAccessLog {
            name: r.get("navn"),
            role: r.get("rolle"),
            method: r.get("method"),
            path: r.get("path"),
            created_at: r.get("created_at"),
        })
        .collect())
}

/// The rows of the platform log that concern one company — what the
/// company's own administrators read. This is the "varslet" requirement:
/// vendor access is visible to the one it touched.
pub async fn platform_access_for_company(
    pool: &PgPool,
    company_id: Uuid,
) -> Result<Vec<PlatformAccessLog>> {
    access_log(pool, "company_id", company_id).await
}

pub async fn platform_access_for_firm(
    pool: &PgPool,
    firm_id: Uuid,
) -> Result<Vec<PlatformAccessLog>> {
    access_log(pool, "firm_id", firm_id).await
}

#[derive(Debug, Clone)]
pub struct PlatformCompany {
    pub id: Uuid,
    pub orgnumber: String,
    pub name: String,
}

pub async fn platform_list_companies(
    pool: &PgPool,
    sok: Option<&str>,
) -> Result<Vec<PlatformCompany>> {
    let rows = sqlx::query(
        "select id, orgnr, name from company
         where $1::text is null or name ilike '%' || $1 || '%' or orgnr like $1 || '%'
         order by name
         limit 100",
    )
    .bind(sok.map(str::trim).filter(|s| !s.is_empty()))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| PlatformCompany {
            id: r.get("id"),
            orgnumber: r.get("orgnr"),
            name: r.get("name"),
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct PlatformFirm {
    pub id: Uuid,
    pub orgnumber: String,
    pub name: String,
    pub kind: String,
}

pub async fn platform_list_firms(pool: &PgPool, sok: Option<&str>) -> Result<Vec<PlatformFirm>> {
    let rows = sqlx::query(
        "select id, orgnr, name, kind from firm
         where $1::text is null or name ilike '%' || $1 || '%' or orgnr like $1 || '%'
         order by name
         limit 100",
    )
    .bind(sok.map(str::trim).filter(|s| !s.is_empty()))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| PlatformFirm {
            id: r.get("id"),
            orgnumber: r.get("orgnr"),
            name: r.get("name"),
            kind: r.get("kind"),
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct PlatformAffiliation {
    /// "selskap" or "byra".
    pub slag: String,
    pub id: Uuid,
    pub name: String,
    pub orgnumber: String,
    pub role: String,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct PlatformUser {
    pub person_id: Uuid,
    pub name: String,
    pub email: Option<String>,
    pub kind: String,
    pub tilknytninger: Vec<PlatformAffiliation>,
}

/// Users across the whole platform, each with the connections that answer
/// "where does this person belong?" — company memberships and byrå
/// memberships. Master data only; nothing here reads a ledger.
pub async fn platform_list_users(pool: &PgPool, sok: Option<&str>) -> Result<Vec<PlatformUser>> {
    let persons = sqlx::query(
        "select id, coalesce(name, oidc_sub) as navn, email, kind from person
         where $1::text is null
            or name ilike '%' || $1 || '%'
            or email ilike '%' || $1 || '%'
         order by navn
         limit 100",
    )
    .bind(sok.map(str::trim).filter(|s| !s.is_empty()))
    .fetch_all(pool)
    .await?;
    let ids: Vec<Uuid> = persons.iter().map(|r| r.get("id")).collect();
    let connections = sqlx::query(
        "select cm.person_id, 'selskap' as slag, c.id, c.name as navn, c.orgnr,
                cm.role as rolle, cm.active as aktiv
         from company_member cm join company c on c.id = cm.company_id
         where cm.person_id = any($1)

         union all

         select fm.person_id, 'byra', f.id, f.name, f.orgnr, fm.role, fm.active
         from firm_member fm join firm f on f.id = fm.firm_id
         where fm.person_id = any($1)

         order by navn",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    Ok(persons
        .iter()
        .map(|p| {
            let person_id: Uuid = p.get("id");
            PlatformUser {
                person_id,
                name: p.get("navn"),
                email: p.get("email"),
                kind: p.get("kind"),
                tilknytninger: connections
                    .iter()
                    .filter(|c| c.get::<Uuid, _>("person_id") == person_id)
                    .map(|c| PlatformAffiliation {
                        slag: c.get("slag"),
                        id: c.get("id"),
                        name: c.get("navn"),
                        orgnumber: c.get("orgnr"),
                        role: c.get("rolle"),
                        active: c.get("aktiv"),
                    })
                    .collect(),
            }
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct PlatformCustomer {
    pub party_id: Uuid,
    pub party_no: String,
    pub name: String,
    pub orgnumber: Option<String>,
    pub email: Option<String>,
    pub company_id: Uuid,
    pub company_name: String,
    pub company_orgnumber: String,
}

/// Customers across all companies, each shown with the company it belongs
/// to. The connection is fixed: a party's id is part of the hash chain, so
/// a customer is never moved between companies — deliberately no function
/// here can do it.
pub async fn platform_list_customers(
    pool: &PgPool,
    sok: Option<&str>,
) -> Result<Vec<PlatformCustomer>> {
    let rows = sqlx::query(
        "select pa.id, pa.party_no, pa.name as navn, pa.orgnr, pa.email,
                c.id as company_id, c.name as company_navn, c.orgnr as company_orgnr
         from party pa
         join company c on c.id = pa.company_id
         where pa.kind = 'kunde'
           and ($1::text is null
                or pa.name ilike '%' || $1 || '%'
                or pa.orgnr like $1 || '%'
                or pa.party_no like $1 || '%'
                or c.name ilike '%' || $1 || '%')
         order by pa.name
         limit 100",
    )
    .bind(sok.map(str::trim).filter(|s| !s.is_empty()))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| PlatformCustomer {
            party_id: r.get("id"),
            party_no: r.get("party_no"),
            name: r.get("navn"),
            orgnumber: r.get("orgnr"),
            email: r.get("email"),
            company_id: r.get("company_id"),
            company_name: r.get("company_navn"),
            company_orgnumber: r.get("company_orgnr"),
        })
        .collect())
}

async fn require_human(pool: &PgPool, person_id: Uuid) -> Result<()> {
    let kind: Option<String> = sqlx::query("select kind from person where id = $1")
        .bind(person_id)
        .fetch_optional(pool)
        .await?
        .map(|r| r.get("kind"));
    match kind.as_deref() {
        None => bail!("ukjent person"),
        Some("menneske") => Ok(()),
        Some(_) => bail!("integrasjoner får tilgang gjennom integrasjonsoppdrag, ikke medlemskap"),
    }
}

/// Assigns a person to a company through the platform path. A NEW
/// membership may be created by support and systemadmin alike; CHANGING an
/// existing one (role change, reactivation) is systemadmin territory —
/// "only System Admins set roles on every user". Every outcome leaves a
/// `company_member_change` row with kilde='plattform', so the company's
/// own tilgangshistorikk shows exactly what the platform did.
pub async fn platform_assign_company(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    role: &str,
    performed_by: Uuid,
    systemadmin: bool,
) -> Result<()> {
    ensure!(
        SELSKAPSROLLER.contains(&role),
        "ukjent rolle «{role}» — plattformen tildeler bare de innebygde"
    );
    require_human(pool, person_id).await?;
    let mut tx = pool.begin().await?;
    let existing = sqlx::query(
        "select role, active from company_member
         where company_id = $1 and person_id = $2
         for update",
    )
    .bind(company_id)
    .bind(person_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (change, from) = match existing {
        None => {
            let inserted = sqlx::query(
                "insert into company_member (company_id, person_id, role)
                 select $1, $2, $3 where exists (select 1 from company where id = $1)",
            )
            .bind(company_id)
            .bind(person_id)
            .bind(role)
            .execute(&mut *tx)
            .await?;
            ensure!(inserted.rows_affected() == 1, "ukjent selskap");
            ("lagt_til", None)
        }
        Some(row) => {
            let from: String = row.get("role");
            let active: bool = row.get("active");
            ensure!(
                systemadmin,
                "medlemskapet finnes fra før — endringer krever systemadmin"
            );
            sqlx::query(
                "update company_member set role = $3, active = true
                 where company_id = $1 and person_id = $2",
            )
            .bind(company_id)
            .bind(person_id)
            .bind(role)
            .execute(&mut *tx)
            .await?;
            (
                if active { "rolle_endret" } else { "reaktivert" },
                Some(from),
            )
        }
    };
    sqlx::query(
        "insert into company_member_change
             (id, company_id, person_id, endring, fra_rolle, til_rolle, utfort_av, kilde)
         values ($1, $2, $3, $4, $5, $6, $7, 'plattform')",
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(person_id)
    .bind(change)
    .bind(from.as_deref())
    .bind(role)
    .bind(performed_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// The byrå twin of [`platform_assign_company`].
pub async fn platform_assign_firm(
    pool: &PgPool,
    firm_id: Uuid,
    person_id: Uuid,
    role: &str,
    performed_by: Uuid,
    systemadmin: bool,
) -> Result<()> {
    ensure!(
        BYRAROLLER.contains(&role),
        "ukjent byrårolle «{role}» — gyldige er admin og ansatt"
    );
    require_human(pool, person_id).await?;
    let mut tx = pool.begin().await?;
    let existing = sqlx::query(
        "select role, active from firm_member
         where firm_id = $1 and person_id = $2
         for update",
    )
    .bind(firm_id)
    .bind(person_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (change, from) = match existing {
        None => {
            let inserted = sqlx::query(
                "insert into firm_member (firm_id, person_id, role)
                 select $1, $2, $3 where exists (select 1 from firm where id = $1)",
            )
            .bind(firm_id)
            .bind(person_id)
            .bind(role)
            .execute(&mut *tx)
            .await?;
            ensure!(inserted.rows_affected() == 1, "ukjent byrå");
            ("lagt_til", None)
        }
        Some(row) => {
            let from: String = row.get("role");
            let active: bool = row.get("active");
            ensure!(
                systemadmin,
                "medlemskapet finnes fra før — endringer krever systemadmin"
            );
            sqlx::query(
                "update firm_member set role = $3, active = true
                 where firm_id = $1 and person_id = $2",
            )
            .bind(firm_id)
            .bind(person_id)
            .bind(role)
            .execute(&mut *tx)
            .await?;
            (
                if active { "rolle_endret" } else { "reaktivert" },
                Some(from),
            )
        }
    };
    sqlx::query(
        "insert into firm_member_change
             (id, firm_id, person_id, endring, fra_rolle, til_rolle, utfort_av, kilde)
         values ($1, $2, $3, $4, $5, $6, $7, 'plattform')",
    )
    .bind(Uuid::new_v4())
    .bind(firm_id)
    .bind(person_id)
    .bind(change)
    .bind(from.as_deref())
    .bind(role)
    .bind(performed_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Counts for the platform console's dashboard. Cheap aggregates over
/// administrative master data — nothing here reads any ledger.
#[derive(Debug, Clone)]
pub struct PlatformOverview {
    pub selskaper: i64,
    pub byraer: i64,
    pub brukere: i64,
    pub integrasjoner: i64,
    pub plattformbrukere: i64,
}

pub async fn platform_overview(pool: &PgPool) -> Result<PlatformOverview> {
    let row = sqlx::query(
        "select
            (select count(*) from company) as selskaper,
            (select count(*) from firm) as byraer,
            (select count(*) from person where kind = 'menneske') as brukere,
            (select count(*) from person where kind = 'integrasjon') as integrasjoner,
            (select count(*) from platform_member
              where valid_from <= current_date and current_date < valid_to)
                as plattformbrukere",
    )
    .fetch_one(pool)
    .await?;
    Ok(PlatformOverview {
        selskaper: row.get("selskaper"),
        byraer: row.get("byraer"),
        brukere: row.get("brukere"),
        integrasjoner: row.get("integrasjoner"),
        plattformbrukere: row.get("plattformbrukere"),
    })
}

/// The facts the abonnement status rule needs, for every company at
/// once. The RULE stays in `regnmed-core::abonnement` — this fetches the
/// same three facts as `abonnement::fakta`, just set-wise, and the API
/// layer folds them into statuses. Billing master data, never balances.
#[derive(Debug, Clone)]
pub struct PlatformSubscription {
    pub company_id: Uuid,
    pub orgnumber: String,
    pub name: String,
    pub created: NaiveDate,
    pub covered_today: bool,
    pub last_end: Option<NaiveDate>,
    /// Plan on the row covering today, if any.
    pub plan: Option<String>,
    /// The current coverage's end, when an oppsigelse has set one.
    pub valid_to: Option<NaiveDate>,
}

pub async fn platform_list_subscriptions(pool: &PgPool) -> Result<Vec<PlatformSubscription>> {
    let rows = sqlx::query(
        "select c.id, c.orgnr, c.name, c.created_at::date as opprettet,
                d.plan, d.valid_to,
                (d.plan is not null) as dekket,
                (select max(a.valid_to) from abonnement a
                  where a.company_id = c.id and a.valid_to is not null
                    and a.valid_to <= current_date) as siste_slutt
         from company c
         left join lateral (
             select a.plan, a.valid_to from abonnement a
              where a.company_id = c.id
                and a.valid_from <= current_date
                and (a.valid_to is null or a.valid_to > current_date)
              order by a.valid_from desc
              limit 1
         ) d on true
         order by c.name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| PlatformSubscription {
            company_id: r.get("id"),
            orgnumber: r.get("orgnr"),
            name: r.get("name"),
            created: r.get("opprettet"),
            covered_today: r.get("dekket"),
            last_end: r.get("siste_slutt"),
            plan: r.get("plan"),
            valid_to: r.get("valid_to"),
        })
        .collect())
}

// ---------------------------------------------------------------------
// Platform settings (migration 0053): what systemadmin decides for the
// whole platform. Insert-only, newest row per key wins.
// ---------------------------------------------------------------------

pub async fn platform_setting(pool: &PgPool, key: &str) -> Result<Option<String>> {
    let value = sqlx::query_scalar(
        "select value from platform_setting
         where key = $1 order by created_at desc limit 1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(value)
}

pub async fn set_platform_setting(
    pool: &PgPool,
    key: &str,
    value: &str,
    set_by: Uuid,
) -> Result<()> {
    sqlx::query("insert into platform_setting (key, value, set_by) values ($1, $2, $3)")
        .bind(key)
        .bind(value)
        .bind(set_by)
        .execute(pool)
        .await?;
    Ok(())
}

/// Coverage history for the drill-down — the abonnement rows as they
/// were written, newest first. Billing master data, never balances.
#[derive(Debug, Clone)]
pub struct DekningsRad {
    pub plan: String,
    pub valid_from: NaiveDate,
    pub valid_to: Option<NaiveDate>,
    pub note: String,
    pub created_by: String,
}

pub async fn coverage_rows(pool: &PgPool, company_id: Uuid) -> Result<Vec<DekningsRad>> {
    let rows = sqlx::query(
        "select plan, valid_from, valid_to, note, created_by
         from abonnement where company_id = $1
         order by valid_from desc, created_at desc
         limit 20",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| DekningsRad {
            plan: r.get("plan"),
            valid_from: r.get("valid_from"),
            valid_to: r.get("valid_to"),
            note: r.get("note"),
            created_by: r.get("created_by"),
        })
        .collect())
}
