//! Subscription packages (plans) as data (migration 0093, docs/abonnement.md).
//!
//! A plan is an insert-only version row; the newest row per slug applies.
//! Prices stay in `abonnement_pris` (dated, append-only), so billing is
//! untouched: this module only describes what a plan is called, who it is
//! for, and what the public price page may list. The limits are descriptive
//! until something enforces them.

use anyhow::{Result, ensure};
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// What the console submits for one plan version.
#[derive(Debug, Clone)]
pub struct PlanInput {
    pub name: String,
    /// `plan` or `addon`.
    pub kind: String,
    pub available: bool,
    pub audience: String,
    pub featured: bool,
    pub listed: bool,
    pub voucher_limit: Option<i32>,
    pub employee_limit: Option<i32>,
    pub integration_limit: Option<i32>,
    pub features: Vec<String>,
    pub sort: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanPrice {
    pub interval: String,
    pub price_ore_per_month: i64,
    pub valid_from: NaiveDate,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub slug: String,
    pub name: String,
    pub kind: String,
    pub available: bool,
    pub audience: String,
    pub featured: bool,
    pub listed: bool,
    pub voucher_limit: Option<i32>,
    pub employee_limit: Option<i32>,
    pub integration_limit: Option<i32>,
    pub features: Vec<String>,
    pub sort: i32,
    pub set_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Companies with coverage on this plan today: a new price for it changes what they pay.
    pub companies: i64,
    /// Newest first; a `year` row's amount is for the whole year.
    pub prices: Vec<PlanPrice>,
}

/// A slug is permanent (it is what `abonnement.plan` points at) and goes
/// into URLs, so it is strict.
pub fn validate_slug(slug: &str) -> Result<(), String> {
    // Same rule as the table check and the console: words of [a-z0-9] joined by single hyphens.
    let ok = !slug.is_empty()
        && slug.len() <= 40
        && slug.split('-').all(|w| {
            !w.is_empty()
                && w.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        });
    if ok {
        Ok(())
    } else {
        Err("plan-id kan bare ha små bokstaver, tall og bindestrek (maks 40 tegn)".into())
    }
}

pub fn validate_input(input: &PlanInput) -> Result<(), String> {
    if !matches!(input.kind.as_str(), "plan" | "addon") {
        return Err("typen må være plan eller addon".into());
    }
    if input.kind == "plan" && !input.available {
        return Err("en plan er alltid tilgjengelig; bare et tillegg kan være «kommer»".into());
    }
    if input.name.trim().is_empty() || input.name.chars().count() > 80 {
        return Err("navnet må være mellom 1 og 80 tegn".into());
    }
    if input.audience.chars().count() > 200 {
        return Err("målgruppen kan være på høyst 200 tegn".into());
    }
    if input.features.len() > 12
        || input
            .features
            .iter()
            .any(|f| f.trim().is_empty() || f.chars().count() > 120)
    {
        return Err("høyst 12 punkter, hver mellom 1 og 120 tegn".into());
    }
    if input.voucher_limit.is_some_and(|v| v <= 0) {
        return Err("bilagsgrensen må være større enn 0, eller tom for ingen grense".into());
    }
    if input.integration_limit.is_some_and(|v| v < 0) {
        return Err("grensen for integrasjoner kan ikke være negativ".into());
    }
    if input.employee_limit.is_some_and(|v| v < 0) {
        return Err("ansattgrensen kan ikke være negativ".into());
    }
    Ok(())
}

/// The kind of an existing slug (from its newest version), if it exists.
pub async fn existing_kind(pool: &PgPool, slug: &str) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "select kind from subscription_plan where slug = $1 order by created_at desc, id desc limit 1",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await?)
}

/// Companies with coverage on `slug` today.
pub async fn companies_on_plan(pool: &PgPool, slug: &str) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "select count(distinct company_id) from abonnement
         where plan = $1 and valid_from <= current_date
           and (valid_to is null or valid_to > current_date)",
    )
    .bind(slug)
    .fetch_one(pool)
    .await?)
}

/// A new version of a plan (or a brand-new plan for a new slug). The kind of a
/// slug is permanent: coverage rows and prices point at it as one or the other.
pub async fn save_plan(pool: &PgPool, slug: &str, input: &PlanInput, by: Uuid) -> Result<()> {
    ensure!(validate_slug(slug).is_ok(), "ugyldig plan-id");
    ensure!(validate_input(input).is_ok(), "ugyldig plan");
    if let Some(kind) = existing_kind(pool, slug).await? {
        ensure!(kind == input.kind, "typen til en plan kan ikke endres");
    }
    sqlx::query(
        "insert into subscription_plan
            (slug, name, kind, available, audience, featured, listed, voucher_limit,
             employee_limit, features, sort, set_by, integration_limit)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(slug)
    .bind(input.name.trim())
    .bind(&input.kind)
    .bind(input.available)
    .bind(input.audience.trim())
    .bind(input.featured)
    .bind(input.listed)
    .bind(input.voucher_limit)
    .bind(input.employee_limit)
    .bind(serde_json::json!(
        input.features.iter().map(|f| f.trim()).collect::<Vec<_>>()
    ))
    .bind(input.sort)
    .bind(by)
    .bind(input.integration_limit)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn plan_exists(pool: &PgPool, slug: &str) -> Result<bool> {
    Ok(
        sqlx::query_scalar("select exists(select 1 from subscription_plan where slug = $1)")
            .bind(slug)
            .fetch_one(pool)
            .await?,
    )
}

/// Why a price row was refused, so the API can answer with a message.
#[derive(Debug, PartialEq, Eq)]
pub enum PriceRefused {
    UnknownPlan,
    BadInterval,
    Negative,
    NoSource,
    /// No retroactive prices: invoicing looks prices up by date, so a
    /// row dated in the past would rewrite what an old month "cost".
    InThePast,
    /// There is already a price for this plan, interval and day.
    Duplicate,
    /// This many companies are on the plan today and the caller did not confirm.
    InUse(i64),
}

/// A new price row. Dated data, append-only: a change is another row,
/// valid from today or later.
pub async fn add_price(
    pool: &PgPool,
    slug: &str,
    interval: &str,
    price_ore: i64,
    from: NaiveDate,
    today: NaiveDate,
    source: &str,
    confirm_existing: bool,
) -> Result<std::result::Result<(), PriceRefused>> {
    if !matches!(interval, "month" | "year") {
        return Ok(Err(PriceRefused::BadInterval));
    }
    if price_ore < 0 {
        return Ok(Err(PriceRefused::Negative));
    }
    if source.trim().is_empty() {
        return Ok(Err(PriceRefused::NoSource));
    }
    if from < today {
        return Ok(Err(PriceRefused::InThePast));
    }
    if !plan_exists(pool, slug).await? {
        return Ok(Err(PriceRefused::UnknownPlan));
    }
    // Invoicing prices by date, so a new row reprices everyone already on the plan.
    let on_plan = companies_on_plan(pool, slug).await?;
    if on_plan > 0 && !confirm_existing {
        return Ok(Err(PriceRefused::InUse(on_plan)));
    }
    let inserted = sqlx::query(
        "insert into abonnement_pris (plan, interval, pris_ore_per_mnd, valid_from, kilde)
         values ($1,$2,$3,$4,$5)",
    )
    .bind(slug)
    .bind(interval)
    .bind(price_ore)
    .bind(from)
    .bind(source.trim())
    .execute(pool)
    .await;
    match inserted {
        Ok(_) => Ok(Ok(())),
        Err(e) if e.as_database_error().and_then(|d| d.code()).as_deref() == Some("23505") => {
            Ok(Err(PriceRefused::Duplicate))
        }
        Err(e) => Err(e.into()),
    }
}

fn features_of(v: serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Every plan's newest version with its full price history, for the console.
pub async fn list_plans(pool: &PgPool) -> Result<Vec<Plan>> {
    list_plans_with(pool, true).await
}

/// `counts` = how many companies are on each plan (the console needs it, the
/// per-company status does not: it reads every coverage row).
pub async fn list_plans_with(pool: &PgPool, counts: bool) -> Result<Vec<Plan>> {
    let rows = sqlx::query(
        "select distinct on (slug) slug, name, kind, available, audience, featured, listed, voucher_limit,
                employee_limit, integration_limit, features, sort, set_by, created_at
         from subscription_plan order by slug, created_at desc, id desc",
    )
    .fetch_all(pool)
    .await?;
    let prices = crate::subscription::list_prices(pool).await?;
    let counts: std::collections::HashMap<String, i64> = if !counts {
        Default::default()
    } else {
        sqlx::query(
            "select plan, count(distinct company_id) n from abonnement
         where valid_from <= current_date and (valid_to is null or valid_to > current_date)
         group by plan",
        )
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| (r.get("plan"), r.get("n")))
        .collect()
    };
    let mut plans: Vec<Plan> = rows
        .iter()
        .map(|r| {
            let slug: String = r.get("slug");
            let companies = counts.get(&slug).copied().unwrap_or(0);
            Plan {
                prices: prices
                    .iter()
                    .filter(|p| p.plan == slug)
                    .map(|p| PlanPrice {
                        interval: p.interval.clone(),
                        price_ore_per_month: p.price_ore_per_month,
                        valid_from: p.valid_from,
                        source: p.source.clone(),
                    })
                    .collect(),
                slug,
                name: r.get("name"),
                kind: r.get("kind"),
                available: r.get("available"),
                audience: r.get("audience"),
                featured: r.get("featured"),
                listed: r.get("listed"),
                voucher_limit: r.get("voucher_limit"),
                employee_limit: r.get("employee_limit"),
                integration_limit: r.get("integration_limit"),
                features: features_of(r.get("features")),
                sort: r.get("sort"),
                set_by: r.get("set_by"),
                created_at: r.get("created_at"),
                companies,
            }
        })
        .collect();
    plans.sort_by(|a, b| (a.sort, &a.slug).cmp(&(b.sort, &b.slug)));
    Ok(plans)
}

#[derive(Debug, Clone, Serialize)]
pub struct Upcoming {
    pub price_ore_per_month: i64,
    pub valid_from: NaiveDate,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicPlan {
    pub slug: String,
    pub name: String,
    pub kind: String,
    pub available: bool,
    pub audience: String,
    pub featured: bool,
    pub voucher_limit: Option<i32>,
    pub employee_limit: Option<i32>,
    pub integration_limit: Option<i32>,
    pub features: Vec<String>,
    /// The month price valid today; `None` when the plan has none yet.
    pub price_ore_per_month: Option<i64>,
    pub in_force: bool,
    pub upcoming: Option<Upcoming>,
}

/// What the public price page may show: listed plans, the month price
/// that applies on `today`, and the next dated change if there is one.
pub async fn public_pricing(pool: &PgPool, today: NaiveDate) -> Result<Vec<PublicPlan>> {
    let plans = list_plans(pool).await?;
    Ok(plans
        .into_iter()
        .filter(|p| p.listed)
        .map(|p| {
            let month: Vec<&PlanPrice> =
                p.prices.iter().filter(|x| x.interval == "month").collect();
            let current = month
                .iter()
                .filter(|x| x.valid_from <= today)
                .max_by_key(|x| x.valid_from);
            let upcoming = month
                .iter()
                .filter(|x| x.valid_from > today)
                .min_by_key(|x| x.valid_from)
                .map(|x| Upcoming {
                    price_ore_per_month: x.price_ore_per_month,
                    valid_from: x.valid_from,
                });
            PublicPlan {
                price_ore_per_month: current.map(|x| x.price_ore_per_month),
                in_force: current.is_some(),
                upcoming,
                slug: p.slug,
                name: p.name,
                kind: p.kind,
                available: p.available,
                audience: p.audience,
                featured: p.featured,
                voucher_limit: p.voucher_limit,
                employee_limit: p.employee_limit,
                integration_limit: p.integration_limit,
                features: p.features,
            }
        })
        .collect())
}

/// The package a company is on today and its limits; `None` in trial or without
/// coverage, which means no limits (the trial is the whole product).
#[derive(Debug, Clone, Serialize)]
pub struct CompanyPlan {
    pub slug: String,
    pub name: String,
    pub voucher_limit: Option<i32>,
    pub employee_limit: Option<i32>,
    pub integration_limit: Option<i32>,
}

pub async fn plan_for_company<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
    company_id: Uuid,
) -> Result<Option<CompanyPlan>> {
    let row = sqlx::query(
        "select a.plan, p.name, p.voucher_limit, p.employee_limit, p.integration_limit
         from abonnement a
         left join lateral (
             select name, voucher_limit, employee_limit, integration_limit
             from subscription_plan where slug = a.plan
             order by created_at desc, id desc limit 1
         ) p on true
         where a.company_id = $1 and a.valid_from <= current_date
           and (a.valid_to is null or a.valid_to > current_date)
         order by a.valid_from desc limit 1",
    )
    .bind(company_id)
    .fetch_optional(ex)
    .await?;
    Ok(row.map(|r| {
        let slug: String = r.get("plan");
        CompanyPlan {
            name: r
                .get::<Option<String>, _>("name")
                .unwrap_or_else(|| slug.clone()),
            slug,
            voucher_limit: r.get("voucher_limit"),
            employee_limit: r.get("employee_limit"),
            integration_limit: r.get("integration_limit"),
        }
    }))
}

/// What a company uses of its limits: vouchers this fiscal year that a person or
/// an integration made (the import journal and the vouchers regnmed posts by
/// itself — depreciation, accruals, payroll runs, reminder fees, year-end — are
/// not counted: the customer should not pay for our automation), active
/// employees and active integration grants.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Usage {
    pub vouchers: i64,
    pub employees: i64,
    pub integrations: i64,
}

pub async fn usage(pool: &PgPool, company_id: Uuid, fiscal_year: i32) -> Result<Usage> {
    let r = sqlx::query(
        "select
            (select count(*) from voucher v join journal j on j.id = v.journal_id
              where v.company_id = $1 and v.fiscal_year = $2 and j.code <> 'IMP'
                and not exists (select 1 from asset_depreciation x where x.voucher_id = v.id)
                and not exists (select 1 from periodisering_run x where x.voucher_id = v.id)
                and not exists (select 1 from payroll_run x where x.voucher_id = v.id)
                and not exists (select 1 from invoice_reminder x where x.voucher_id = v.id)
                and not exists (select 1 from arsavslutning x where x.voucher_id = v.id)) vouchers,
            (select count(*) from employee
              where company_id = $1 and (ansatt_til is null or ansatt_til >= current_date)) employees,
            (select count(*) from integration_grant
              where company_id = $1 and revoked_by is null
                and (valid_to is null or valid_to > current_date)) integrations",
    )
    .bind(company_id)
    .bind(fiscal_year)
    .fetch_one(pool)
    .await?;
    Ok(Usage {
        vouchers: r.get("vouchers"),
        employees: r.get("employees"),
        integrations: r.get("integrations"),
    })
}

/// Refuses one more of something the package caps. Runs inside the caller's
/// transaction after locking the company row, so two concurrent requests
/// cannot both take the last place. Bookkeeping itself is never capped.
pub async fn ensure_room(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    what: Capped,
) -> Result<()> {
    sqlx::query("select 1 from company where id = $1 for update")
        .bind(company_id)
        .execute(&mut **tx)
        .await?;
    let Some(plan) = plan_for_company(&mut **tx, company_id).await? else {
        return Ok(());
    };
    let (limit, count_sql, noun_one, noun_many) = match what {
        Capped::Employees => (
            plan.employee_limit,
            "select count(*) from employee where company_id = $1
               and (ansatt_til is null or ansatt_til >= current_date)",
            "ansatt i lønn",
            "ansatte i lønn",
        ),
        Capped::Integrations => (
            plan.integration_limit,
            "select count(*) from integration_grant where company_id = $1
               and revoked_by is null and (valid_to is null or valid_to > current_date)",
            "integrasjon eller agent",
            "integrasjoner og agenter",
        ),
    };
    let Some(limit) = limit else { return Ok(()) };
    let used: i64 = sqlx::query_scalar(count_sql)
        .bind(company_id)
        .fetch_one(&mut **tx)
        .await?;
    ensure!(
        used < i64::from(limit),
        "Pakken «{}» har plass til {} {}. Bytt til en større pakke under Abonnement for å legge til flere.",
        plan.name,
        limit,
        if limit == 1 { noun_one } else { noun_many }
    );
    Ok(())
}

/// Why a company does not fit a package's hard limits today, if it does not.
/// Pure, so the rule is tested without a database.
pub fn misfit(
    plan_name: &str,
    employee_limit: Option<i32>,
    integration_limit: Option<i32>,
    usage: &Usage,
) -> Option<String> {
    if let Some(l) = employee_limit
        && usage.employees > i64::from(l)
    {
        return Some(format!(
            "Selskapet har {} ansatte i lønn, og pakken «{plan_name}» har plass til {l}. Avslutt ansatte eller velg en større pakke.",
            usage.employees
        ));
    }
    if let Some(l) = integration_limit
        && usage.integrations > i64::from(l)
    {
        return Some(format!(
            "Selskapet har {} integrasjoner og agenter, og pakken «{plan_name}» har plass til {l}. Trekk tilbake tilganger eller velg en større pakke.",
            usage.integrations
        ));
    }
    None
}

/// Whether the company fits `slug`'s hard limits as it is today.
pub async fn fits(
    pool: &PgPool,
    company_id: Uuid,
    slug: &str,
    fiscal_year: i32,
) -> Result<Option<String>> {
    let Some(plan) = list_plans_with(pool, false)
        .await?
        .into_iter()
        .find(|p| p.slug == slug)
    else {
        return Ok(None);
    };
    let usage = usage(pool, company_id, fiscal_year).await?;
    Ok(misfit(
        &plan.name,
        plan.employee_limit,
        plan.integration_limit,
        &usage,
    ))
}

/// The plan of the company's most recent coverage, current or ended: what a
/// grandfathered customer may restart on after a lapse.
pub async fn last_plan(pool: &PgPool, company_id: Uuid) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "select plan from abonnement where company_id = $1
         order by valid_from desc, created_at desc limit 1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?)
}

#[derive(Debug, Clone, Copy)]
pub enum Capped {
    Employees,
    Integrations,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> PlanInput {
        PlanInput {
            name: "Liten".into(),
            kind: "plan".into(),
            available: true,
            audience: String::new(),
            featured: false,
            listed: true,
            voucher_limit: Some(150),
            employee_limit: Some(1),
            integration_limit: Some(0),
            features: vec!["Alle funksjoner".into()],
            sort: 1,
        }
    }

    #[test]
    fn slugs_are_strict() {
        assert!(validate_slug("liten").is_ok());
        assert!(validate_slug("pro-2027").is_ok());
        for bad in [
            "",
            "Liten",
            "-x",
            "x-",
            "a--b",
            "a b",
            "æ",
            "a_b",
            &"x".repeat(41),
        ] {
            assert!(validate_slug(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_company_over_the_hard_limits_does_not_fit() {
        let u = Usage {
            vouchers: 9999,
            employees: 5,
            integrations: 3,
        };
        assert!(
            misfit("Liten", Some(1), Some(0), &u)
                .unwrap()
                .contains("5 ansatte")
        );
        assert!(
            misfit("X", None, Some(2), &u)
                .unwrap()
                .contains("3 integrasjoner")
        );
        assert!(
            misfit("Stor", Some(25), None, &u).is_none(),
            "vouchers never decide fit"
        );
        assert!(misfit("Basis", None, None, &u).is_none());
    }

    #[test]
    fn an_addon_may_be_coming_but_a_plan_may_not() {
        let mut i = input();
        i.kind = "addon".into();
        i.available = false;
        assert!(validate_input(&i).is_ok());
        i.kind = "plan".into();
        assert!(validate_input(&i).is_err());
        i.kind = "bundle".into();
        i.available = true;
        assert!(validate_input(&i).is_err());
    }

    #[test]
    fn a_plan_must_be_sane() {
        assert!(validate_input(&input()).is_ok());
        let mut i = input();
        i.name = "  ".into();
        assert!(validate_input(&i).is_err());
        let mut i = input();
        i.voucher_limit = Some(0);
        assert!(validate_input(&i).is_err());
        let mut i = input();
        i.employee_limit = Some(-1);
        assert!(validate_input(&i).is_err());
        let mut i = input();
        i.features = vec!["x".into(); 13];
        assert!(validate_input(&i).is_err());
        let mut i = input();
        i.features = vec![String::new()];
        assert!(validate_input(&i).is_err());
        let mut i = input();
        i.voucher_limit = None;
        i.employee_limit = Some(0);
        assert!(
            validate_input(&i).is_ok(),
            "no limit, and zero employees, are valid"
        );
    }
}
