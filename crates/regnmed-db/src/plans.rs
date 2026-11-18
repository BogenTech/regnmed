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
    pub features: Vec<String>,
    pub sort: i32,
    pub set_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Newest first; a `year` row's amount is for the whole year.
    pub prices: Vec<PlanPrice>,
}

/// A slug is permanent (it is what `abonnement.plan` points at) and goes
/// into URLs, so it is strict.
pub fn validate_slug(slug: &str) -> Result<(), String> {
    let ok = !slug.is_empty()
        && slug.len() <= 40
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !slug.starts_with('-');
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
    if input.employee_limit.is_some_and(|v| v < 0) {
        return Err("ansattgrensen kan ikke være negativ".into());
    }
    Ok(())
}

/// A new version of a plan (or a brand-new plan for a new slug).
pub async fn save_plan(pool: &PgPool, slug: &str, input: &PlanInput, by: Uuid) -> Result<()> {
    ensure!(validate_slug(slug).is_ok(), "ugyldig plan-id");
    ensure!(validate_input(input).is_ok(), "ugyldig plan");
    sqlx::query(
        "insert into subscription_plan
            (slug, name, kind, available, audience, featured, listed, voucher_limit,
             employee_limit, features, sort, set_by)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
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
    let rows = sqlx::query(
        "select distinct on (slug) slug, name, kind, available, audience, featured, listed, voucher_limit,
                employee_limit, features, sort, set_by, created_at
         from subscription_plan order by slug, created_at desc, id desc",
    )
    .fetch_all(pool)
    .await?;
    let prices = crate::subscription::list_prices(pool).await?;
    let mut plans: Vec<Plan> = rows
        .iter()
        .map(|r| {
            let slug: String = r.get("slug");
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
                features: features_of(r.get("features")),
                sort: r.get("sort"),
                set_by: r.get("set_by"),
                created_at: r.get("created_at"),
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
                features: p.features,
            }
        })
        .collect())
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
            features: vec!["Alle funksjoner".into()],
            sort: 1,
        }
    }

    #[test]
    fn slugs_are_strict() {
        assert!(validate_slug("liten").is_ok());
        assert!(validate_slug("pro-2027").is_ok());
        for bad in ["", "Liten", "-x", "a b", "æ", "a_b", &"x".repeat(41)] {
            assert!(validate_slug(bad).is_err(), "{bad:?}");
        }
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
