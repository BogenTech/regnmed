//! Platform roles: systemadmin and support (docs/auth.md §8).
//!
//! This is the deliberately bounded exception to "no access path crosses a
//! company boundary". A platform role reaches administrative master data —
//! persons, memberships, customer registers — and NOTHING in any company's
//! ledger. The company-scoped guard (`tilgang::krev`) and the access
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
pub struct AktivPlattformRolle {
    pub rolle: String,
    pub valid_to: NaiveDate,
}

/// The strongest active platform role for a person, if any. Exclusive
/// `valid_to`, like engagements: revocation takes effect immediately.
pub async fn active_platform_role(
    pool: &PgPool,
    person_id: Uuid,
) -> Result<Option<AktivPlattformRolle>> {
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
    Ok(row.map(|r| AktivPlattformRolle {
        rolle: r.get("rolle"),
        valid_to: r.get("valid_to"),
    }))
}

#[derive(Debug, Clone)]
pub struct PlattformMedlem {
    pub id: Uuid,
    pub person_id: Uuid,
    pub navn: String,
    pub epost: Option<String>,
    pub rolle: String,
    pub valid_from: NaiveDate,
    pub valid_to: NaiveDate,
    pub notat: String,
    pub aktiv: bool,
}

pub async fn list_platform_members(pool: &PgPool) -> Result<Vec<PlattformMedlem>> {
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
        .map(|r| PlattformMedlem {
            id: r.get("id"),
            person_id: r.get("person_id"),
            navn: r.get("navn"),
            epost: r.get("email"),
            rolle: r.get("rolle"),
            valid_from: r.get("valid_from"),
            valid_to: r.get("valid_to"),
            notat: r.get("notat"),
            aktiv: r.get("aktiv"),
        })
        .collect())
}

/// Grants a platform role. `granted_by` is NULL only from the CLI
/// bootstrap — the first systemadmin cannot arrive through an API that
/// only systemadmins may call.
pub async fn grant_platform_role(
    pool: &PgPool,
    person_id: Uuid,
    rolle: &str,
    valid_to: NaiveDate,
    notat: &str,
    granted_by: Option<Uuid>,
) -> Result<Uuid> {
    ensure!(
        rolle == "systemadmin" || rolle == "support",
        "ukjent plattformrolle «{rolle}» — gyldige er systemadmin og support"
    );
    ensure!(
        valid_to > Utc::now().date_naive(),
        "valid_to må ligge i framtiden — plattformroller er tidsbegrensede"
    );
    ensure!(
        !notat.trim().is_empty(),
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
    .bind(rolle)
    .bind(valid_to)
    .bind(notat.trim())
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
pub async fn person_by_email(pool: &PgPool, epost: &str) -> Result<Option<Uuid>> {
    let norm = epost.trim().to_lowercase();
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
    rolle: &str,
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
    .bind(rolle)
    .bind(method)
    .bind(path)
    .bind(company_id)
    .bind(firm_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct PlattformInnsyn {
    pub navn: String,
    pub rolle: String,
    pub method: String,
    pub path: String,
    pub created_at: DateTime<Utc>,
}

async fn innsyn(pool: &PgPool, column: &str, id: Uuid) -> Result<Vec<PlattformInnsyn>> {
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
        .map(|r| PlattformInnsyn {
            navn: r.get("navn"),
            rolle: r.get("rolle"),
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
) -> Result<Vec<PlattformInnsyn>> {
    innsyn(pool, "company_id", company_id).await
}

pub async fn platform_access_for_firm(
    pool: &PgPool,
    firm_id: Uuid,
) -> Result<Vec<PlattformInnsyn>> {
    innsyn(pool, "firm_id", firm_id).await
}

#[derive(Debug, Clone)]
pub struct PlattformSelskap {
    pub id: Uuid,
    pub orgnr: String,
    pub name: String,
}

pub async fn platform_list_companies(
    pool: &PgPool,
    sok: Option<&str>,
) -> Result<Vec<PlattformSelskap>> {
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
        .map(|r| PlattformSelskap {
            id: r.get("id"),
            orgnr: r.get("orgnr"),
            name: r.get("name"),
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct PlattformByra {
    pub id: Uuid,
    pub orgnr: String,
    pub name: String,
    pub kind: String,
}

pub async fn platform_list_firms(pool: &PgPool, sok: Option<&str>) -> Result<Vec<PlattformByra>> {
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
        .map(|r| PlattformByra {
            id: r.get("id"),
            orgnr: r.get("orgnr"),
            name: r.get("name"),
            kind: r.get("kind"),
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct PlattformTilknytning {
    /// "selskap" or "byra".
    pub slag: String,
    pub id: Uuid,
    pub navn: String,
    pub orgnr: String,
    pub rolle: String,
    pub aktiv: bool,
}

#[derive(Debug, Clone)]
pub struct PlattformBruker {
    pub person_id: Uuid,
    pub navn: String,
    pub epost: Option<String>,
    pub kind: String,
    pub tilknytninger: Vec<PlattformTilknytning>,
}

/// Users across the whole platform, each with the connections that answer
/// "where does this person belong?" — company memberships and byrå
/// memberships. Master data only; nothing here reads a ledger.
pub async fn platform_list_users(pool: &PgPool, sok: Option<&str>) -> Result<Vec<PlattformBruker>> {
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
            PlattformBruker {
                person_id,
                navn: p.get("navn"),
                epost: p.get("email"),
                kind: p.get("kind"),
                tilknytninger: connections
                    .iter()
                    .filter(|c| c.get::<Uuid, _>("person_id") == person_id)
                    .map(|c| PlattformTilknytning {
                        slag: c.get("slag"),
                        id: c.get("id"),
                        navn: c.get("navn"),
                        orgnr: c.get("orgnr"),
                        rolle: c.get("rolle"),
                        aktiv: c.get("aktiv"),
                    })
                    .collect(),
            }
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct PlattformKunde {
    pub party_id: Uuid,
    pub party_no: String,
    pub navn: String,
    pub orgnr: Option<String>,
    pub epost: Option<String>,
    pub company_id: Uuid,
    pub company_navn: String,
    pub company_orgnr: String,
}

/// Customers across all companies, each shown with the company it belongs
/// to. The connection is fixed: a party's id is part of the hash chain, so
/// a customer is never moved between companies — deliberately no function
/// here can do it.
pub async fn platform_list_customers(
    pool: &PgPool,
    sok: Option<&str>,
) -> Result<Vec<PlattformKunde>> {
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
        .map(|r| PlattformKunde {
            party_id: r.get("id"),
            party_no: r.get("party_no"),
            navn: r.get("navn"),
            orgnr: r.get("orgnr"),
            epost: r.get("email"),
            company_id: r.get("company_id"),
            company_navn: r.get("company_navn"),
            company_orgnr: r.get("company_orgnr"),
        })
        .collect())
}

async fn krev_menneske(pool: &PgPool, person_id: Uuid) -> Result<()> {
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
    rolle: &str,
    utfort_av: Uuid,
    systemadmin: bool,
) -> Result<()> {
    ensure!(
        SELSKAPSROLLER.contains(&rolle),
        "ukjent rolle «{rolle}» — plattformen tildeler bare de innebygde"
    );
    krev_menneske(pool, person_id).await?;
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
    let (endring, fra) = match existing {
        None => {
            let inserted = sqlx::query(
                "insert into company_member (company_id, person_id, role)
                 select $1, $2, $3 where exists (select 1 from company where id = $1)",
            )
            .bind(company_id)
            .bind(person_id)
            .bind(rolle)
            .execute(&mut *tx)
            .await?;
            ensure!(inserted.rows_affected() == 1, "ukjent selskap");
            ("lagt_til", None)
        }
        Some(row) => {
            let fra: String = row.get("role");
            let aktiv: bool = row.get("active");
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
            .bind(rolle)
            .execute(&mut *tx)
            .await?;
            (if aktiv { "rolle_endret" } else { "reaktivert" }, Some(fra))
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
    .bind(endring)
    .bind(fra.as_deref())
    .bind(rolle)
    .bind(utfort_av)
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
    rolle: &str,
    utfort_av: Uuid,
    systemadmin: bool,
) -> Result<()> {
    ensure!(
        BYRAROLLER.contains(&rolle),
        "ukjent byrårolle «{rolle}» — gyldige er admin og ansatt"
    );
    krev_menneske(pool, person_id).await?;
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
    let (endring, fra) = match existing {
        None => {
            let inserted = sqlx::query(
                "insert into firm_member (firm_id, person_id, role)
                 select $1, $2, $3 where exists (select 1 from firm where id = $1)",
            )
            .bind(firm_id)
            .bind(person_id)
            .bind(rolle)
            .execute(&mut *tx)
            .await?;
            ensure!(inserted.rows_affected() == 1, "ukjent byrå");
            ("lagt_til", None)
        }
        Some(row) => {
            let fra: String = row.get("role");
            let aktiv: bool = row.get("active");
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
            .bind(rolle)
            .execute(&mut *tx)
            .await?;
            (if aktiv { "rolle_endret" } else { "reaktivert" }, Some(fra))
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
    .bind(endring)
    .bind(fra.as_deref())
    .bind(rolle)
    .bind(utfort_av)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Counts for the platform console's dashboard. Cheap aggregates over
/// administrative master data — nothing here reads any ledger.
#[derive(Debug, Clone)]
pub struct PlattformOversikt {
    pub selskaper: i64,
    pub byraer: i64,
    pub brukere: i64,
    pub integrasjoner: i64,
    pub plattformbrukere: i64,
}

pub async fn platform_overview(pool: &PgPool) -> Result<PlattformOversikt> {
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
    Ok(PlattformOversikt {
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
pub struct PlattformAbonnement {
    pub company_id: Uuid,
    pub orgnr: String,
    pub name: String,
    pub opprettet: NaiveDate,
    pub dekket_i_dag: bool,
    pub siste_slutt: Option<NaiveDate>,
    /// Plan on the row covering today, if any.
    pub plan: Option<String>,
    /// The current coverage's end, when an oppsigelse has set one.
    pub valid_to: Option<NaiveDate>,
}

pub async fn platform_list_subscriptions(pool: &PgPool) -> Result<Vec<PlattformAbonnement>> {
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
        .map(|r| PlattformAbonnement {
            company_id: r.get("id"),
            orgnr: r.get("orgnr"),
            name: r.get("name"),
            opprettet: r.get("opprettet"),
            dekket_i_dag: r.get("dekket"),
            siste_slutt: r.get("siste_slutt"),
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
